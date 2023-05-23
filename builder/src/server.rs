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
                events: broadcast::channel(64).0,
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
                    anyhow::bail!(anyhow!(e).context("failed to accept on listener"));
                }
            };

            let service = Service {
                inner: self.inner.clone(),
            };
            let connection = http.serve_connection(stream, service);
            tokio::task::spawn(async move {
                if let Err(e) = connection.await {
                    // These "errors" are unavoidable with SSE. There's no point in logging them.
                    if e.is_incomplete_message() {
                        return;
                    }
                    log::error!("{:?}", anyhow!(e).context("connection error"));
                }
            });
        }
    }

    pub(crate) fn update(&self, event: notify::Event) {
        drop(self.inner.events.send(Arc::new(event)));
    }
}

#[derive(Clone)]
struct Service {
    inner: Arc<Inner>,
}

struct Inner {
    path: Box<Path>,
    not_found_path: PathBuf,
    events: broadcast::Sender<Arc<notify::Event>>,
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
        if req.uri().path() == "/watch" {
            self.respond_sse(req).await
        } else {
            self.respond_file(req).await
        }
    }

    async fn respond_sse(&self, req: http::Request<hyper::Body>) -> http::Response<hyper::Body> {
        let mut paths = Vec::new();
        let Some(query) = req.uri().query() else {
            return bad_request("no query parameters in URI");
        };
        for (key, value) in form_urlencoded::parse(query.as_bytes()) {
            if key != "path" {
                return bad_request("query key was not `path`");
            }
            paths.push(match self.fs_path(&value).await {
                Some((path, _metadata)) => path,
                // TODO: Live-reload on the 404 page as well
                None => return self.not_found().await,
            });
        }

        let (mut sender, body) = hyper::Body::channel();

        let mut receiver = self.inner.events.subscribe();

        tokio::spawn(async move {
            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        if event.paths.iter().any(|changed_path| {
                            paths
                                .iter()
                                .any(|watched_path| changed_path.ends_with(watched_path))
                        }) {
                            break;
                        }
                    }
                    // Server shutdown; exit without reloading
                    Err(broadcast::error::RecvError::Closed) => return,
                    Err(broadcast::error::RecvError::Lagged(_)) => break,
                }
            }
            if let Err(e) = sender.send_data("data:\n\n".into()).await {
                // A closed channel is OK; it just means the client has disconnected.
                if e.is_closed() {
                    return;
                }
                let e = anyhow!(e).context("failed to send data to SSE stream");
                log::error!("{e:?}");
            }
        });

        http::Response::builder()
            .header("content-type", "text/event-stream")
            .body(body)
            .unwrap()
    }

    async fn respond_file(&self, req: http::Request<hyper::Body>) -> http::Response<hyper::Body> {
        let Some((path, metadata)) = self.fs_path(req.uri().path()).await else {
            return self.not_found().await;
        };

        let content_type = match path.extension().and_then(OsStr::to_str) {
            Some("html") => "text/html",
            Some("xml") => "application/xml",
            Some("css") => "text/css",
            Some("js") => "application/javascript",
            Some("png") => "image/png",
            Some("ico") => "image/x-icon",
            Some("svg") => "image/svg+xml",
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

fn bad_request(err: impl Display) -> http::Response<hyper::Body> {
    let mut bytes = BytesMut::new();
    write!((&mut bytes).writer(), "{err}").unwrap();
    http::Response::builder()
        .status(http::StatusCode::BAD_REQUEST)
        .body(hyper::Body::from(bytes.freeze()))
        .unwrap()
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

use anyhow::anyhow;
use anyhow::Context as _;
use bytes::BufMut as _;
use bytes::BytesMut;
use fn_error_context::context;
use hyper::http;
use std::convert::Infallible;
use std::ffi::OsStr;
use std::fmt::Display;
use std::fs;
use std::future::Future;
use std::io;
use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task;
use std::task::Poll;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
