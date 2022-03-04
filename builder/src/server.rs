use ::{
    anyhow::{anyhow, bail, Context as _},
    fn_error_context::context,
    hyper::http,
    std::{
        collections::HashMap,
        convert::Infallible,
        ffi::OsStr,
        fs,
        future::Future,
        io,
        path::{Path, PathBuf},
        pin::Pin,
        sync::{Arc, Mutex},
        task::{self, Poll},
    },
    tokio::{net::TcpListener, sync::Notify},
};

#[derive(Clone)]
pub(crate) struct Server {
    inner: Arc<Inner>,
}

impl Server {
    pub(crate) fn new(path: &Path) -> Self {
        Self {
            inner: Arc::from(Inner {
                path: Box::from(path),
                not_found_path: path.join("404.html"),
                path_waiters: Mutex::new(HashMap::new()),
            }),
        }
    }

    #[context("failed to run server on port {port}")]
    pub(crate) fn listen(&self, port: u16) -> anyhow::Result<Infallible> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("failed to start tokio runtime")?
            .block_on(self.listen_async(port))
    }

    async fn listen_async(&self, port: u16) -> anyhow::Result<Infallible> {
        let listener = TcpListener::bind(("0.0.0.0", port))
            .await
            .context("failed to bind TCP listener")?;

        log::info!("now listening on http://localhost:{port}");

        let http = hyper::server::conn::Http::new();

        loop {
            let stream = match listener.accept().await {
                Ok((stream, _)) => stream,
                Err(e) if CONNECTION_ERROR_KINDS.contains(&e.kind()) => continue,
                Err(e) => {
                    bail!(anyhow!(e).context("failed to accept on listener"));
                }
            };

            let service = Service {
                inner: self.inner.clone(),
            };
            let connection = http.serve_connection(stream, service);
            tokio::task::spawn(async move {
                if let Err(e) = connection.await.context("connection error") {
                    log::error!("{e:?}");
                }
            });
        }
    }

    pub(crate) fn update(&self, event: &notify::Event) {
        let waiters = self.inner.path_waiters.lock().unwrap();
        for path in &event.paths {
            if let Some(notify) = waiters.get(path) {
                notify.notify_waiters();
            }
        }
    }
}

#[derive(Clone)]
struct Service {
    inner: Arc<Inner>,
}

struct Inner {
    path: Box<Path>,
    not_found_path: PathBuf,
    path_waiters: Mutex<HashMap<PathBuf, Arc<Notify>>>,
}

impl tower_service::Service<http::Request<hyper::Body>> for Service {
    type Response = http::Response<hyper::Body>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
    fn call(&mut self, req: http::Request<hyper::Body>) -> Self::Future {
        let this = self.clone();
        Box::pin(async move { Ok(this.respond(req).await) })
    }
}

impl Service {
    async fn respond(&self, req: http::Request<hyper::Body>) -> http::Response<hyper::Body> {
        let (path, metadata) = match self.fs_path(req.uri().path()).await {
            Some(t) => t,
            None => return self.not_found().await,
        };

        if req
            .headers()
            .get("accept")
            .and_then(|s| s.to_str().ok())
            .map_or(false, |val| val.contains("text/event-stream"))
        {
            // TODO: By only supporting one URL, dependency assets like `common.css` end up
            // untracked. This endpoint should accept a list of files to track.

            // TODO: Live-reload on the 404 page as well
            let (mut sender, body) = hyper::Body::channel();

            let notify = {
                let mut path_waiters = self.inner.path_waiters.lock().unwrap();
                let notify = path_waiters.entry(path).or_insert_with(Default::default);
                notify.clone()
            };

            tokio::spawn(async move {
                notify.notified().await;
                if let Err(e) = sender.send_data("data:\n\n".into()).await {
                    let e = anyhow!(e).context("failed to send data to SSE stream");
                    log::error!("{e:?}");
                }
            });

            http::Response::builder()
                .header("content-type", "text/event-stream")
                .body(body)
                .unwrap()
        } else {
            let content_type = match path.extension().and_then(OsStr::to_str) {
                Some("html") => "text/html",
                Some("css") => "text/css",
                Some("js") => "application/javascript",
                Some("png") => "image/png",
                Some("ico") => "image/x-icon",
                _ => "application/octet-stream",
            };

            let body = match *req.method() {
                http::Method::HEAD => hyper::Body::empty(),
                http::Method::GET => {
                    let result = tokio::task::spawn_blocking(|| fs::read(path)).await;
                    match result.unwrap() {
                        Ok(bytes) => hyper::Body::from(bytes),
                        Err(e) => {
                            log::error!("{:?}", anyhow!(e).context("failed to read file"));
                            return self.not_found().await;
                        }
                    }
                }
                _ => return method_not_allowed(),
            };

            http::Response::builder()
                .header("content-length", metadata.len())
                .header("content-type", content_type)
                .header("cache-control", "no-store")
                .body(body)
                .unwrap()
        }
    }

    async fn fs_path(&self, path: &str) -> Option<(PathBuf, fs::Metadata)> {
        let path = path.trim_start_matches('/');
        let decoded = percent_encoding::percent_decode_str(path)
            .decode_utf8()
            .ok()?;

        let mut path = self.inner.path.to_path_buf();
        for part in decoded.split('/') {
            if part.starts_with('.') || part.contains('\\') {
                return None;
            }
            path.push(part);
        }

        if !path.starts_with(&*self.inner.path) {
            return None;
        }

        let task = tokio::task::spawn_blocking(move || {
            let metadata = match fs::metadata(&*path) {
                Ok(metadata) if !metadata.is_file() => {
                    path.push("index.html");
                    fs::metadata(&*path)?
                }
                Ok(metadata) => metadata,
                Err(e) if e.kind() == io::ErrorKind::NotFound && path.extension().is_none() => {
                    path.set_extension("html");
                    fs::metadata(&*path)?
                }
                Err(e) => return Err(e),
            };
            Ok((path, metadata))
        });
        task.await.unwrap().ok()
    }

    async fn not_found(&self) -> http::Response<hyper::Body> {
        let response = http::Response::builder().status(http::StatusCode::NOT_FOUND);

        let inner = self.inner.clone();
        match tokio::task::spawn_blocking(move || fs::read(&inner.not_found_path)).await {
            Ok(Ok(bytes)) => response
                .header("content-type", "text/html")
                .body(hyper::Body::from(bytes)),
            _ => response.body(hyper::Body::empty()),
        }
        .unwrap()
    }
}

fn method_not_allowed() -> http::Response<hyper::Body> {
    http::Response::builder()
        .status(http::StatusCode::METHOD_NOT_ALLOWED)
        .body(hyper::Body::empty())
        .unwrap()
}

const CONNECTION_ERROR_KINDS: [io::ErrorKind; 3] = [
    io::ErrorKind::ConnectionRefused,
    io::ErrorKind::ConnectionAborted,
    io::ErrorKind::ConnectionReset,
];
