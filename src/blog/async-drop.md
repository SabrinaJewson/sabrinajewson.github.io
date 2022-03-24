{
	"published": "2022-03-24"
}

# Async destructors, async genericity and completion futures

The main focus of this article
will be on attempting to design a system to support
asynchronous destructors in the Rust programming language,
figuring the exact semantics of them
and resolving any issues encountered along the way.
By side effect, it also designs a language feature called "async genericity"
which enables supporting blocking and asynchronous code with the same codebase,
as well as designing a system for completion-guaranteed futures
to be added to the language.

## Why async destructors? { #why-async-destructors }

Async destructors, at a high level,
would allow types to run code with `.await`s inside it when they are dropped.
This enables cleanup code to actually perform I/O,
giving much more freedom in the extent
to which resources can be properly cleaned up.
One notable use case for this is implementing the TLS protocol, in which:
> ```
> Each party MUST send a "close_notify" alert before closing its write
> side of the connection, unless it has already sent some error alert.
> ```
([RFC 8446](https://datatracker.ietf.org/doc/html/rfc8446#section-6.1)).
In order to make sure that this requirement is consistently fulfilled,
TLS implementations should be able to send this alert when the `TlsStream` type is dropped -
and if all I/O is done asynchronously,
this requires asynchronous destructors.

Currently,
this kind of cleanup is generally managed
by methods like [`poll_shutdown`] and [`poll_close`]:
asynchronous functions that can optionally be called by the user
if they want the type to be cleanly disposed of.
However, this approach has several limitations:
- There is no way to statically guarantee that the method isn't called twice,
	that's up to the user.
- There is no way to statically guarantee that the method is called at all -
	it can be very easy to forget.
- Calling it at the lifecycle end of each value is cumbersome boilerplate,
	and would ideally not be necessary.
- It only works on types that actually implement `AsyncWrite`.
	If your type is not actually a byte stream, too bad.

Clearly we need a better solution than this.
So let's look at some practical examples
to work out what features we'd need to improve the situation.

## Async drop after future cancellation { #async-drop-after-future-cancellation }

Let's start simple, with this trivial function:

```rs
async fn wait_then_drop_stream(_stream: TlsStream) {
	time::sleep(Duration::from_secs(10)).await;
}
```

It's an asynchronous function that takes ownership of a `TlsStream`,
sleeps for 10 seconds,
then implicitly drops it at the end.
The most obvious characteristic we want of this function is that
the TLS stream should perform graceful `close_notify` shutdown after the 10 seconds.
However there's also a slightly more subtle but equally important one:
because in Rust every future is implicitly made cancellable at `.await` points,
the same graceful shutdown should also happen if the future is cancelled.
For example, suppose the function is used like this:

```rs
let handle = task::spawn(wait_then_drop_stream(some_tls_stream));
time::sleep(Duration::from_secs(5)).await;
handle.cancel();
```

Just because we cancel the task overall
doesn't mean we suddenly want to
sidestep the regular graceful shutdown
and have the TLS stream finish in an unclean manner -
in fact, we almost never want that.
So somehow we need a way to register async operations to occur after a future is cancelled,
in order to support running the graceful shutdown code in there.
How do we do that?

As it turns out, with async destructors in the language that becomes quite easy:
since future cancellation is signalled to the future is via calling its destructor,
the future can simply itself have an async destructor and run the cleanup code in there.
The precise semantics of this would work in a very similar way
to how synchronous destruction works today:
drop each of the local variables in reverse order
(and this critically includes the `_stream` variable).
<!--_-->

## Hidden awaits { #hidden-awaits }

A second question we have to answer is what happens
when async destruction _itself_ is cancelled -
for example,
you might be in the middle of dropping a TLS stream,
but at the same time your task suddenly gets aborted.
To demonstrate this problem,
take a look at this function:

```rs
async fn assign_stream(target: &mut TlsStream, source: TlsStream) {
	*target = source; // Async destructor is implicitly called!
	println!("1");
	async { println!("2") }.await;
	println!("3");
	yield_now().await;
	println!("4");
}
```

It assigns the `source` TLS stream to the `target` TLS
stream (dropping the old `source` stream in the process),
then prints out numbers 1 to 4.
Under normal circumstances,
this task would just run from top to bottom
and always print out every number;
but when cancellation gets involved, things become more complicated.
If cancellation were to happen during the assignment of `source` to `target`,
the language now has to decide what to do
with the rest of the code -
should it run it to the end?
Should it immediately exit?
Should it run only _some_ of it?

There are three main categories of option worth talking about here:
"abort now" designs, "never abort" designs and "delayed abort" designs.
Each one has both advantages and drawbacks,
which are explored in detail below.

### "Abort now" designs { #abort-now }

Under these designs, none of the four prints in the code above are guaranteed to run -
if the assignment is aborted,
it will exit the future as soon as possible
while performing the minimum amount of cleanup
(i.e. just running destructors and nothing else).

There are three variants of this design,
differing slightly in when they require `.await` to be specified:
1. Sometimes await:
	Under this design,
	`=` is kept to never require an `.await`
	and async function calls are kept to always require an `.await`.
	This mostly keeps things the same way as they are:
	no special new syntax is introduced, and no major breaking changes are made.

	To get a feel for how this looks,
	here is a non-trivial "real world" async function
	implemented using it:


	```rs
	async fn handle_stream(mut stream: TlsStream) -> Result<()> {
		loop {
			match read_message(&mut stream).await? {
				Message::Redirect(address) => {
					stream = connect(address).await?;
					// The below line isn't guaranteed to run even if
					// redirection succeded, since the future could be
					// cancelled during the drop of the old `TlsStream`.
					log::info!("Redirected");
				}
				Message::Exit => break,
			}
		}
	}
	```

	It does introduce a footgun
	as it will no longer be obvious at which points control flow can exit a function.
	It can also be considered inconsistent
	as some suspend points require an `.await` while others don't,
	despite the fact that there is no meaningful semantic difference between the two kinds.

1. Never await:
	To resolve that inconsistency,
	this design removes `.await`s altogether,
	making all cancellation points completely invisible.
	Adapting our example from before, it would look like:

	```rs
	async fn handle_stream(mut stream: TlsStream) -> Result<()> {
		loop {
			match read_message(&mut stream)? {
				Message::Redirect(address) => {
					stream = connect(address)?;
					log::info!("Redirected");
				}
				Message::Exit => break,
			}
		}
	}
	```

	Aside from the technical issues of removing `.await`
	(is it done recursively?
	does it make implementing `Future` a breaking change?
	are async blocks made redundant?
	et cetera)
	and the backwards compatibility/churn issue,
	this has the same footgun issue as the previous option but turned up to the extreme -
	it would now be basically impossible to carefully manage where cancellations can occur
	and most users would end up having to treat cancellation more as a `pthread_kill`
	than a helpful control flow construct.

1. Always await:
	On the flip side, this design makes `.await`s mandatory everywhere.
	Assignments to a value with an asynchronous destructor
	must be done with a new `=.await` operator instead of plain `=`,
	and values cannot implicitly fall out of scope
	but must instead be explicitly `drop`ped by the user.
	Once again returning to the `handle_stream` example:

	```rs
	async fn handle_stream(mut stream: TlsStream) -> Result<()> {
		loop {
			match read_message(&mut stream).await? {
				Message::Redirect(address) => {
					stream =.await connect(address).await?;
					log::info!("Redirected");
				}
				Message::Exit => break,
			}
		}
		drop(stream).await;
	}
	```

	This is the only option of the three
	to definitively avoid the "implicit cancel" footgun,
	but it's still not ideal
	as it ends up introducing new weird-looking syntax
	and makes writing async code pretty verbose.

All three of these variants end up with pretty significant drawbacks -
fundamentally,
it's pretty incompatible with the current async syntax and model.
So if aborting is so tricky to support,
what if we could sidestep the problem by avoiding it altogether?

### "Never abort" designs { #never-abort }

This design category eliminates implicit cancellation entirely from the language.
Futures would, much like synchronous functions,
run from linearly top to bottom
without the possibility of caller-induced early exit
(of course, panics can still cause early exit to happen).
This means that all of `1`, `2`, `3` _and_ `4` are guaranteed to be printed
in the `assign_stream` function shown at the start of this section,
since at no point is code execution ever allowed to stop.
[This approach has been proposed by Carl Lerche previously][6 ways],
if you want to read more about it.

Much like the "abort now" category, it has three sub-designs,
"always await", "sometimes await" and "never await"
depending on where `.await` is deemed to be necessary.
Much of the same arguments listed up there apply,
although there is no longer the issue of the footgun
caused by potential cancellation points being implicit
so it is mostly a question of weighing up
consistency, breakage and new syntax.

This is another highly consistent approach,
however it comes with the major downside
of throwing away the very useful tool that is implicit cancellation contexts.
While it is definitely possible for cancellation to be implemented as a library feature
(see [`CancellationToken`] and [`StopToken`])
and I want that to be an option for use cases that need it,
most of the time having an implicit context is far more useful
since it is less verbose and requires much less boilerplate to make use of.
I would hate to see otherwise infallible functions become fallible,
or an enormous migration effort to add cancellation token parameters to every function.

One argument Carl Lerche used to support his point was an example code snippet
in which future cancellation combined with `select!` turned out to be a footgun.
But as Yoshua Wuyts argued in [Futures Concurrency III],
the primary problem in code like that is the confusing semantics of `select!`
and not the cancellation behaviour of futures.
Ultimately,
I do not believe cancellation to be problematic enough
to warrant removing it from the language.
Although this approach's consistency and its parallel with blocking code is nice,
cancellation is still useful
and there are ways to combine it with async destructors
that don't introduce footguns.

Note that even with the other options,
adding async destructors to the language would make it trivial
to create a combinator that executes futures in a "no-cancellation" mode
if such semantics are desired -
see [appendix D](#uncancellable-futures) for more.

### "Delayed abort" designs { #delayed-abort }

Unlike the previous two designs,
these approaches try to fully embrace the syntactical difference
between assigning and falling out of scope,
which don't require an `.await`,
and calling an async function,
which does.
When the caller attempts to cancel the future
during one of the former operations,
the future will actually continue to run for a short while afterwards
until it is able to reach one of the latter operations and properly exit.

This immediately solves the main set of problems that plagued the "abort now" designs
without going to the extreme that never-abort did:
there is no footgun as cancellation points are never implicitly introduced,
no new syntax is added and no major breaking changes are made,
and there is now a definite reason _why_ `=` doesn't need `.await` but calling functions does.

However, it is not perfect.
It effectively introduces two different kinds of suspend point
which behave pretty differently,
an inconsistency not present with "abort now" and "never abort" designs.
Additionally, it means that
if you call a wrapper function around the `=` operator or call `drop` manually,
it has subtly different semantics from using the built-in language behaviour
since it changes what kind of suspend point it is.
This is probably unexpected and unintuitive for most users.

There are three variations of this design,
depending on when the code stops running:

1. Abort before first await:
	Code will continue to run after cancellation of an operation like `=`
	until the next point at which `.await` occurs,
	at which point the outer future will promptly exit
	without even polling the inner future once.
	In the `assign_stream` example,
	that means that `1` is guaranteed to be printed,
	but everything after that isn't.
1. Abort after first await:
	As with the previous one,
	but the future will be polled once
	(only to have its result discarded and the outer future to exit).
	In our example, that means `1` and `2` are guranteed to be printed,
	but not anything beyond that.
1. Abort at first suspend:
	The outer future will abort the first time a future which it `.await`s
	returns `Poll::Pending` when it is polled.
	In the example code, this will force all of `1`, `2` and `3` to be printed,
	but not `4` since `[rs] yield_now()` causes a suspend point to occur.
	This is the most similar to how future cancellation works today,
	because cancellation cannot currently appear to happen without a suspend point
	(it still can't with the above proposals,
	but it appears to because `async {}.await` potentially exits control flow).
	From the future's perspective,
	this behaves is exactly as if the caller had just waited
	and then attempted cancellation later on.

Although they might seem very similar,
with the first two approaches
an extremely subtle but very important paradigm shift is made:
`.await` changes its meaning
from being a "might suspend" operator
to a "might halt" or "might abort" operator,
since `async {}.await;` is now able to cause computation to suddenly stop.
This is a small difference, but ends up very problematic
as we now have to answer a whole host of new questions:
- If `.await` is just about cancellation,
	should we allow omitting it to call async functions while forbidding cancellation?
- Should we allow calling _synchronous_ functions with `.await`
	to introduce cancellation points around them?
- Should we introduce plain `await;` statements to introduce those cancellation points,
	equivalent to `async {}.await;`?

Phrased another way, we open ourselves up to this table existing
whose empty boxes will come across as obvious holes:

| | Caller can't cancel | Caller can cancel |
| - | - | - |
| **Callee can't cancel** | `foo()` | ? |
| **Callee can cancel** | ? | `foo().await` |

I don't think that's a situation we want to be in.
The third approach avoids the whole situation altogether
by tying abort opportunities to suspend points,
removing the need for the second column in that table
and thus closing those holes.

Additionally,
the third variant is less of a breaking change
because code that previously relied
on the immediately-completing parts of an `async` operation not being able to abort
won't have to adjust their expectations.
Technically it's still non-breaking either way
because no existing code uses asynchronous destructors,
but it allows programmers to keep their mental model which is important too.

Because of all these reasons,
I am in favour using a delayed abort design with abort-at-first-suspend:
it would require little migration effort,
avoids footguns
and I don't think is too surprising for users.
The rest of this post will be written assuming that design is chosen.

## Async drop in a sync function { #async-drop-in-a-sync-function }

Perhaps the hardest problem any async drop design has to face
is what happens when a type with an async destructor gets dropped in a synchronous context.
Consider this code:

```rust
fn sync_drop_stream(_stream: TlsStream) {}
```

The synchronous function declared takes a TLS stream as a parameter.
It must do something with the stream it has been given
since it has ownership and there's no return value to pass it back to the caller,
but it can't use a regular asynchronous drop because it is a synchronous function.
So what can it do?
In [withoutboats' post on this subject](https://without.boats/blog/poll-drop/#the-non-async-drop-problem)
they hypothesized two options:

> 1. Call it's non-async destructor, like every other type.
> 1. Introduce some kind of executor to the runtime (probably just `block_on`) to call as part of the drop glue.

To me, both solutions seem pretty bad.
Solution 2 is obviously unworkable for the reasons Boats' outlined,
but I believe solution 1 is far more of a footgun than it appears.
Many many functions from the standard library become essentially off-limits,
so not only do you not get their ergonomics in well-written code
it would be very easy to create bug-ridden code too,
simply by calling any function like [`Option::insert`] on a TLS stream.

My alternative solution is to forbid that code from compiling entirely.
For a type to be dropped in a synchronous context it must implement a certain trait,
and this just wouldn't be implemented for `TlsStream` and similar types.
Therefore, barring using of an explicit `close_unclean` method on `TlsStream`,
it becomes totally impossible to cause an unclean TLS close from anywhere,
eliminating an entire category of bugs.

This approach is not without its difficulties -
in fact, it has more of them than the others
and lots of this article will be simply dedicated to figuring them out.
But ultimately, I do believe it to a better solution
for the sake of those stronger static guarantees.

## Panic checks { #panic-checks }

I mentioned that this design would forbid at compile time
async drop types being dropped in a synchronous context.
So, seems easy right?
Just detect when the compiler would run the destructor for each value
and error out if it's invalid.

```rs
// Error
fn bad(stream: TlsStream) {
	println!("{:?}", stream.protocol_version());
	// Implicitly dropped here: error!
}
// OK
fn good(stream: TlsStream) -> TlsStream {
	println!("{:?}", stream.protocol_version());
	stream
}
```

Except...it's not so simple.
Because at nearly every point in a program, it is possible for the thread to panic,
and if that happens unwinding might start to occur
and if _that_ happens you need to drop all the local variables in scope
but you can only do that if they have a synchronous destructor!
So really the compiler ought to forbid
_any_ usage of values with an asynchronous destructor in a synchronous context
since panics can always happen and mess things up.

```rs
// Error
fn bad(stream: TlsStream) -> TlsStream { stream }
```

But that doesn't work either.
The usage of types with an asynchronous destructor in a synchronous context
is absolutely necessary in many circumstances,
for example `TlsStream::close_unclean` which takes `self` or `block_on` which takes a future.
What the compiler actually needs to enforce is then slightly more relaxed:
While a value that cannot be synchronously dropped is held in scope,
no operations that might panic can occur.
"Operations that might panic" here includes calling any function
or triggering any operator overload.
It only doesn't include simple things like
constructing a struct or tuple,
accessing a type's field (without overloaded `Deref`),
matching,
returning,
or any other built-in and trivial operation.

```rs
// Error
fn bad(stream: TlsStream) -> TlsStream {
	println!("{:?}", stream.protocol_version());
	stream
}
// OK
fn good(stream: TlsStream) -> TlsStream { stream }
```

This rule is quite limited,
but actually provides all the tools necessary for dealing with this situation.
It is particularly effective when combined with `ManuallyDrop`:
because `ManuallyDrop` skips running the destructor of a type,
it is always able to be synchronously dropped even if the type inside isn't.
So as long as the first might-panic operation you do upon obtaining one of these values
is calling `ManuallyDrop::new` on it,
the compiler will allow you to do anything you like
since the burden has effectively been shifted to _you_
to drop the value if you want.
What's more,
`ManuallyDrop::new` itself doesn't have to be implemented with any compiler magic -
since all it does is execute a struct expression and return it,
it passes the panic check just fine.

## Unwinding in async { #unwinding-in-async }

Now that we've looked at what unwinding looks like in a synchronous context,
let's see what it looks like in an asynchronous one.
It should be easier because this time we're actually allowed to await on each value's destruction.

```rs
async fn unwinds(_stream: TlsStream) {
	panic!();
}
```

Sticking with the principle of forbidding ungraceful TLS stream shutdown entirely,
it makes sense for the future to catch this panic
and then asynchronously drop everything in scope like it usually would,
before eventually propagating the panic to the caller.

For parity with synchronous code,
while performing these asynchronous drops
[`std::thread::panicking`] would return `true`
and similarly panicking again would result in an abort.
Actually storing the in-flight panic in the future is easy:
simply store an optional pointer that is the `[rs] Box<dyn Any + Send>` returned by `catch_unwind`,
ready to be passed to `resume_unwind` later.

Unfortunately, those functions [aren't available in `no_std` environments yet](https://github.com/rust-lang/rfcs/issues/2810)
so for now the compiler will probably have to use a workaround
like aborting or leaking the values -
or maybe implementing async destructors could be forbidden entirely on `[rs] #![no_std]`.
If that issue is ever resolved it would be possible to improve the handling
to something more useful.

There is one big issue with this approach however, and that is unwind safety.
Unwind safety is the idea that panics in code can cause shared data structures to enter a logically invalid state,
so whenever you are given the opportunity to observe the world after a panic
it should be checked that you know that that might happen.
This is regulated by two traits, [`UnwindSafe`] and [`RefUnwindSafe`],
which provide the necessary infrastructure to check all of this at compile time.

Implemented simply, this proposal would trivially break that concept:

```rs
#[derive(Clone, Copy, PartialEq, Eq)]
enum State { Valid, Invalid }

let state = Cell::new(State::Valid);

let task = pin!(async {
	let stream = some_tls_stream;
	state.set(State::Invalid);
	panic!();
	state.set(State::Valid);
});
let _ = task.poll(&mut cx);

// Now the task is panicking and polling the TLS stream...

// But we can observe the invalid state!
assert_eq!(state.get(), State::Invalid);
```

So what do we do?
Well, we have a few options:

1. Require that all local variables in async contexts are `UnwindSafe`.
	This would prevent the above code from compiling because `&Cell<T>` is `!UnwindSafe`.
1. Have compiler-generated `async {}` types only implement `Future` when `Self: UnwindSafe`.
	This is mostly the same as the first option, it just causes an error later in compilation.
1. Ignore unwind safety entirely -
	it's already kind of useless because `std::thread::spawn` doesn't require `F: UnwindSafe`
	and that can already be used to witness broken invariants.
	The system as a whole is definitely one of the more confusing and less understood parts of `std`,
	and it usually just amounts to slapping `AssertUnwindSafe` on everything until rustc is happy
	while not actually considering the implications.
1. Have async panics always cause synchronous drops of locals.
	This would force a sync drop option on types
	where it might not even make logical sense to have one,
	and async panic handling would permanently be done suboptimally.

Personally, I'm quite in favour of option 3 - ignoring unwind safety entirely.
I can't think of a time where it has actually been useful for me or prevented a bug,
but of course your mileage may vary
([I know `rust-analyzer` has been saved by unwind safety at least once](https://github.com/rust-lang/chalk/issues/260)).
I'm also open to option 1, although it could end up being quite a pain.

## `poll_drop_ready` { #poll-drop-ready }

In the now-closed [RFC 2958](https://github.com/rust-lang/rfcs/pull/2958),
withoutboats proposed the following design for implementing asynchronous destructors:

```rust
trait Drop {
	fn drop(&mut self);

	fn poll_drop_ready(&mut self, cx: &mut Context<'_>) -> Poll<()> {
		Poll::Ready(())
	}
}
```

Under this design,
dropping a type would be a simple matter of
forwarding to `poll_drop_ready` inside the future's `poll` function
until it returns `[rs] Poll::Ready(())` and execution can continue.
Types would need to hold all state they need to use for destruction inside the type itself.

But this design comes with one _major_ drawback that I haven't seen mentioned so far:
it breaks `Vec`'s three-pointer layout guarantee.
The problem is that `Vec`, when destroyed, needs to drop each of its elements in order.
So with an approach like `poll_drop_ready`,
it would need to keep track of how many elements it has destroyed so far within the `Vec` itself,
since it isn't allowed to introduce any new external state during destruction.
It can't use any existing fields to do this -
`ptr`, `len` and `capacity` are all necessary to keep around -
therefore the only other option is adding a new field,
but [Rust already guarantees](https://doc.rust-lang.org/stable/std/vec/struct.Vec.html#guarantees) that `Vec` will never do that.

It's not like there aren't potential solutions to this,
like hardcoding `Vec`'s async drop code into the language
or only making it four `usize`s for async-drop types.
But both of those are a hack,
and to me appear to just be working around a more fundamental problem with the design.

So how do we avoid this?
Well, we have to allow types to hold state - _new_ state - in their asynchronous destructors.
Such a design was [rejected by withoutboats](https://without.boats/blog/poll-drop/#the-destructor-state-problem)
for two reasons:

1. The resulting future can be unexpectedly `!Send`.
1. It doesn't play well with trait objects.

I don't believe the first problem to be particularly bad,
as if a type's asynchronous destructor ends up being `!Send`
that simply forms part of the type's public API,
similarly to how the type itself being `Send` is.
And in generic contexts,
since `Send` implementations leak all over the place anyway
the `Send`ness of destructors can too:
it would be up to the user to provide a type with a `Send` destructor
if they want the resulting future to be `Send`.

Trait objects definitely pose a larger challenge -
since the new state is of variable size,
it's not possible to stack-allocate it anywhere
like we usually would
with non-type-erased types.
But this isn't a problem that needs to be immediately solved:
it's possible to just forbid `dyn` trait objects with asynchronous destructors for now,
and potentially fill in this gap later.
Since users can always create user-space workarounds for this feature,
it's not urgent to attempt to stabilize a solution immediately.
Additionally
because it's a problem shared with all async traits, not just async destructors,
if a general solution is found for those
it would end up working for this too.

## Function implicit bounds { #function-implicit-bounds }

Now we need to begin to consider how async drop works in generic code.
In particular,
when will a generic parameter enforce that a type does or does not support synchronous drop?

Within the current edition, it is essential that backward compatibility is maintained.
Therefore, we can't suddenly force `T: ?Drop` on any existing function or implementation,
synchronous or asynchronous
since they could very well be relying on synchronous drop support.
If asynchronous drop is to be supported at all by an API,
they must have to explicitly opt in to it
([more on this later](#relaxed-drop-bounds)).
All generic parameters and associated types without that opt-in
would default to requiring a synchronous drop in every context.

To illustrate how this would work, here is an implementation of `FromIterator` for `Option`
annotated with the implicit bounds:

```rs
impl<A, V> FromIterator<Option<A>> for Option<V>
where
	// A: Drop,
	// V: Drop,
	V: FromIterator<A>,
{
	fn from_iter<I>(iter: I) -> Self
	where
		I: IntoIterator<Item = Option<A>>,
		// I: Drop,
		// No `I::IntoIter: Drop` bound is implied here since
		// that's provided by the IntoIterator trait already.
	{
		iter.into_iter().scan((), |_, item| item).collect()
	}
}
```

As a side note, I'm using `T: Drop` syntax to mean "supports synchronous drop".
Unfortunately, that is counterintuitively _not_ what `T: Drop` currently means,
nor does it mean "the type [`needs_drop`]";
instead, it is satisfied only if there a literal `[rs] impl Drop` block for the type,
making the bound entirely useless
in any actual code.
But let's ignore that and assume the more sensible meaning for now.

We get a lot more freedom when considering the next edition,
and we can start relaxing the defaults of those bounds to something more commonly useful.
As long as the standard library provides an adequate set of utilities for dealing with async drop types
migrating should be painless.

Let's look at a few simple examples to try and work out what these defaults should actually be.

```rs
fn sync_drops_a_value<T>(v: T) {}
fn sync_takes_a_ref<T>(v: &T) {}
fn sync_drops_a_clone<T: Clone>(v: &T) { v.clone(); }
async fn async_drops_a_value<T>(v: T) {}
```

`sync_drops_a_value` and `sync_drops_a_clone` should probably compile as-is
and not work with async drop types.
Similarly, `async_drops_a_value` should obviously work with async drop types,
because of course async destructors would be supported in an asynchronous context.
At first glance it looks like `sync_takes_a_ref` can follow suit -
after all, it's not trying to drop anything -
but in practicality it can't, because the compiler shouldn't have to look into its function body
to determine whether it actually does something like `sync_drops_a_clone` does or not.
While that situation is unfortunate,
it is not all bad because as it turns out the extra restriction does not matter in most cases,
since users can often add an extra reference to the type to bridge the gap.

```rust
fn takes_a_ref<T /* implied to require not-async-drop */>(val: &T) { /* ... */ }

let stream: TlsStream = /* ... */;
takes_a_ref(&stream); // doesn't work, since TlsStream is async-drop
takes_a_ref(&&stream); // does work, since &TlsStream is not async-drop
```

Normally, a double reference functions totally equivalently to a single one,
so this shouldn't be a too big problem.
And as older APIs gradually migrate to new syntax it becomes less and less of one.

So past the next edition
all synchronous functions would implicitly bound each generic parameter by `T: Drop`
and all asynchronous functions would use the async equivalent.
While this doesn't cover the desired behaviour 100% of the time,
it covers the majority of cases
and that's all that's needed for a default -
explicit bounds can be used whereever necessary.

Inherent functions follow much the same idea.
Consider this example:

```rust
struct Wrapper<T>(T);

impl<T> Wrapper<T> {
	fn some_sync_method(self) {}
	fn ref_method(&self) {}
	async fn some_async_method(self) {}
}
```

With all the implicit bounds made explicit, it would look like this:

```rs
struct Wrapper<T>(T);

impl<T> Wrapper<T> {
	fn some_sync_method(self) where T: Drop {}
	fn ref_method(&self) where T: Drop {}
	async fn some_async_method(self) where T: AsyncDrop {}
}
```

There is one small addition though:
because of the frequency of wanting to define several synchronous methods that don't care about drop,
one can specify relaxed bounds on the `[rs] impl` block itself
and have it apply to every function inside of it.
This would be useful for defining many of the `Option` methods:

```rs
impl<T: ?Drop> Option<T> {
	pub fn is_some(&self) -> bool { /* ... */ }
	pub fn is_none(&self) -> bool { /* ... */ }
	pub fn as_ref(&self) -> Option<&T> { /* ... */ }
	pub fn as_mut(&mut self) -> Option<&mut T> { /* ... */ }
	// et cetera
}
```

The choices of the exact syntax for this is discussed more later.

## Drop supertrait { #drop-supertrait }

The following code compiles today:

```rs
pub trait Foo {
	fn consumes_self(self) {}
}
```

If any declared trait didn't imply `Drop` as a supertrait,
then we would have a breaking change
as there would no longer be a guarantee that `self` can be dropped like that.
Ultimately, I would like to follow in the path of `Sized`
and have `Foo: Drop` _never_ implied
so that the above code would need an explicit `[rs] where Self: Drop` bound,
but until then that code must desugar like so:

```rs
pub trait Foo: Drop {
	fn consumes_self(self) {}
}
```

And everything can compile again.

It's also possible
that we could introduce some more complex rules about this in the current edition,
like "the supertrait is only implied if there are any default methods";
but they would only help in a small number of cases
and it would be easier to just convince users to use the next edition.

## Async genericity { #async-genericity }

With the current suggestions taken alone,
although async drop will be supported
it would be rather inconvenient
since almost no existing standard library APIs would support it.
Just to show how difficult it would be to use,
here are some functions that wouldn't work with async drop types:

- `Option::insert`,
	since it can drop the old value in the `Option`.
- Many `HashMap` functions: `insert`, `entry`, etc
	since they call methods of user-supplied generics
	which can always panic.
- `Vec::push`,
	since it's synchronous
	and can panic if the `Vec`'s length exceeds `isize::MAX`.
- `Box::new`,
	since it's possible that allocation will be allowed to panic.

One potential option
is to introduce `_async` variants of each of these functions
that are `[rs] async fn`s.
When dealing with async-drop types,
you'd call `[rs] vec.push_async(item).await;` instead of `[rs] vec.push(item);`
and `[rs] Box::new_async(value).await` instead of `[rs] Box::new(value)`.
However this would nearly double the API surface of the standard library
and lead to a large amount of code duplication.
This is obviously undesirable, so what can we do about it?

One potential path forward is a feature known as async overloading,
[previously proposed by Yoshua Wuyts](https://blog.yoshuawuyts.com/async-overloading/).
The idea is that synchronous functions can be overloaded by asynchronous ones,
allowing `Vec::push_async` and `Vec::push` to effectively share the same namespace,
and have the correct function be chosen based on context.

While this does solve the first problem of the doubled API surface quite neatly,
it does not however solve the second problem of code duplication -
one would still have to write two copies of nearly-identical code
for an async and sync implementation of the same algorithm.
And it comes with its own problems too,
such as needing a good way to force one particular overload to be chosen of multiple possibilities.

My alternative idea is what I will refer to as async genericity.
Unlike async overloading which has two separate functions with different bodies,
under async genericity
the async and sync equivalents of one function share a body that works for both.
The compiler can then monomorphize this into two separate functions,
just like it does for generic parameters.
The correct version will be chosen at call site depending on the traits
the given generic parameters implement.
It is, to some extent, colourless async.

## Inspiration from `const` { #inspiration-from-const }

I'd like to take inspiration from [the work on `[rs] const fn`](https://github.com/rust-lang/rust/issues/67792)
which faces a similar problem to the one we're facing now:
how can one function be written that works for multiple modes (async/sync, const/non const)?
A simple example of that is `drop`:

```rs
const fn drop<T: ~const Drop>(_x: T) {}
```

This function can be treated as "expanding" into two separate functions:

```rs
const fn drop_const<T: const Drop>(_x: T) {}
fn drop_non_const<T>(_x: T) {}
```

Where the correct one will be chosen at call site
depending on whether `T` can be dropped in `[rs] const` contexts.
`[rs] const Drop` is a compiler-generated `Drop` subtrait
which has all the same methods as `Drop`,
but converted to `[rs] const fn`s.
This `[rs] const` modifier can actually be applied to any trait
to automatically make it `[rs] const`: `[rs] const Iterator`, `[rs] const Add` et cetera.
You can read more about this in [its pre-RFC](https://internals.rust-lang.org/t/pre-rfc-revamped-const-trait-impl-aka-rfc-2632/15192),
I won't go into the details here.

I will use this as a starting point for the async generics design.
It might look something like this:

```rs
~async fn drop<T>(_x: T) {}
```

The `T: ~async Drop` bound is implied,
like how `T: async Drop` would be implied in normal `[rs] async fn`s.
It "expands" to:

```rs
async fn drop_async<T>(_x: T) {}
fn drop_sync<T>(_x: T) {}
```

In cases where there are multiple generic parameters, like for example:

```rs
~async fn drop_pair<A, B>(_: A, _: B) {}
```

The synchronous version is only possible
when _all_ parameters implement the synchronous version of the trait.

```rs
// `A: async Drop, B: async Drop`
async fn drop_pair_async<A, B>(_: A, _: B) {}

// `A: Drop, B: Drop`
fn drop_pair_sync<A, B>(_: A, _: B) {}
```

If the function is being called where `A: Drop` but `B: async Drop`,
the async version will be selected
since `A: Drop` implies `A: async Drop` already.

If an `~async fn` is declared with _no_ generic parameters
that have an `~async` bound,
then it's actually totally equivalent to a synchronous function
and should probably be warned against by rustc.

One important aspect to note is that `[rs] async` is somewhat the opposite of `[rs] const`.
While a non-`[rs] const` function can always be substituted for a `[rs] const` one,
the inverse is true of `async`:
an `[rs] async` function can always be substituted for a sync one
but not the other way around.
This means that while `[rs] const Trait` is a subtrait of `Trait`
(fewer types implement it than just `Trait`),
`[rs] async Trait` is a supertrait of `Trait`
(more types implement it than just `Trait`).
Or in other words, `[rs] const Trait: Trait: async Trait`.

Another important impact of this system is that, unlike with `const`,
upgrading an implementation from `async Trait` to `Trait` is a breaking change
since the methods will now by default be synchronous instead of asynchronous,
so you'll get errors whereever you previously were using `.await`.
Of course, the actual number of use cases is universally increased, not reduced
(passing it to a function that accepts `async Trait` still works,
and the methods will still require `.await` there)
but direct callers will need to modify their code to have it build.
However this should not be a large problem
since it's generally well known up front whether something will need async or not.

Another option would be to have `async Trait` and `Trait` be treated as two entirely separate traits,
with no inherent connection between the two.
This has the advantage of preventing mistakes
like using `std::fs::File` in an asynchronous function at compile time
(since `std::fs::File` would _not_ implement `async Read`),
but overall I do not think that to be worth it:
1. Users can end up making the mistake anyway, just by calling a concrete blocking function
	like `[rs] .metadata()` on a `Path`
	or `[rs] std::thread::sleep`.
	It would only help prevent a small number of cases.
1. It is not always a mistake;
	sometimes it _is_ useful to run blocking code in an asynchronous context,
	if for example one wants to mix asynchronous and blocking function calls
	on a blocking worker thread.
1. Sometimes whether an operation will _actually_ block is only known dynamically,
	for example reading from a TCP stream -
	if it the stream is in [non-blocking mode][tcp nonblocking]
	(which is explicitly a supported use case by the standard library)
	it should be fine to call it from `async` code.
1. By default types like `[rs] Vec<u8>`
	(whose `Write` implementation is neither asynchronous nor blocking,
	and thus can be used in both contexts)
	would end up being exclusively synchronous.
	To support both, it would have to write out boilerplate code
	to implement both `async Trait` and `Trait` separately,
	or we'd have to introduce _another_ new piece of syntax to share an implementation.

	It gets worse when considering `Drop` -
	every non-generic type implementing that trait would have to migrate to this new syntax
	to even be usable at all in asynchronous contexts
	(or we could special-case `Drop` to have shared implementations,
	but I can't think of a strong reason
	why `Drop` should be treated so differently
	from everything else).
1. Having the traits be separate rather increases the complexity of the system overall.

## Relaxed drop bounds { #relaxed-drop-bounds }

We introduced implicit default `Drop` bounds in a [previous section](#function-implicit-bounds);
now that we have some actual syntax for async drop (`async Drop`)
the question is how those bounds can be relaxed for functions that allow it.

I'd first like to introduce a new concept in this section: the `?Drop` bound.
This bound can be considered the initial one before implicit bounds are added,
and it imposes absolutely no requirements on to what extent the type supports being dropped.
There would not be any situation in which this bound is necessary over `async Drop`,
since the least "droppable" a type can be is `async Drop` -
applying it only takes abilities away from the implementor
while giving none to the caller.
But it is still important to have
because it avoids panic-check-passing synchronous functions that don't care at all about `async`
(`mem::replace`, `any::type_name`, `Option::map` etc)
from having to write `async` in their signature to be general.
It would feel rather strange for them to declare `<T: async Drop>` or something
when they actually don't drop the type asynchronously at all.
It also enables future extensions into more kinds of drop
which [may be useful](#linear-types).

All functions have a stronger default bound for generic parameters than `?Drop`,
and that can be relaxed to `?Drop` in much the same way as the other implied bound in Rust, `Sized`:
by adding `?Drop` as a trait bound in the parameter list or in the where clause.
Like with `Sized` it only accepts the simple cases, so `?Drop` cannot be used as a supertrait
(it is [the default anyway](#drop-supertrait))
or as a bound on types other than a literal type parameter.
There is a slight inconsistency here in that
`?Drop` is used even when the implied bound isn't actually `Drop`,
because it could be in reality `async Drop`;
so in a way it should really be `?async Drop` if the outer function is `[rs] async`
and only `?Drop` if the outer function is sync.
But since `?Drop` is shorter, more consistent and unambiguous anyway
there's no strong reason not to use it.

When relaxing bounds to something weaker than the default but stronger than `?Drop`,
(particularly, setting them to `async Drop` in a synchronous function)
the most obvious option is to support the trait name directly -
use `T: async Drop`
to support `T` not implementing any of the `Drop` subtraits
(`Drop`, `const Drop`),
but requiring it to implement `async Drop`.
However this approach ends up being quite problematic
because unlike `?Drop` whose unique syntax excuses it from only supporting a few special cases,
`async Drop` is also a trait like any other
and so must be supported in the general case like any other.

What this means is that having `T: async Drop` implicitly also relax a `Drop` bound
breaks down in more complex cases
(such as when it's implied through a supertrait,
or transitively via a bound in the `where` clause applied to another type)
leading to inconsistent behaviour
and confusing semantics.

Instead, Rust should take the consistent approach
of _allowing_
(but potentially warning against)
bounds like `T: async Drop` on a synchronous function,
but not giving them any effect unless they're _also_ paired with `?Drop`.
Since `Drop` implies `async Drop`,
adding `async Drop` in a synchronous function is a tautology
and only by taking away the initial `Drop` bound does it have a meaning.

The only problem with this approach is its verbosity:
`T: ?Drop + async Drop` is quite the mouthful to express one concept.
It's possible that Rust could introduce some syntax sugar to make it shorter,
the only difficulty is what the actual syntax of that would be
while remaining clear and unambiguous.
I'm very much open to suggestions here.

## Synchronous opt-out { #synchronous-opt-out }

While blindly turning every method in the trait `const` works most of the time for `const Trait`s,
it doesn't end up working so well for `async Trait`s.
In particular, there are quite a few methods that would benefit from always being synchronous
whether the outer trait is considered asynchronous or not, for example:
- `Iterator::size_hint` and `ExactSizeIterator::len`:
	These methods should be O(1) and not perform I/O,
	so there's no reason to have them be `async`.
- `Iterator::{step_by, chain, zip, map, filter, enumerate, ...}`:
	These functions just construct a type and return it, no asynchronity here.
- `Read::{by_ref, bytes, chain, take}`:
	More trivial functions that just construct a type.
- `BufRead::consume`:
	Any I/O done by the `BufRead` should occur in `fill_buf` and
	all `consume` should do is move around a couple numbers.
	Hence, it should be always synchronous.

So evidently trait definitions need to be able to control what their `async` form would look like.
Having any kind of default chosen by the Rust compiler would be a bad idea,
because even without thinking about `async` code,
just by writing a single trait
you'd have already chosen and stabilized an `async` API.
Plus, it's not like many traits need to have async equivalents -
it's mostly just `Iterator`, I/O traits, functions and `Drop` that matter.
Therefore I think it is best to have `async Trait` support be an opt-in
by the trait declarer.

The syntax to declare one of these traits
can be something along the lines of
`trait ~async Foo`,
`~async trait Foo`, or
`async trait Foo` -
I don't have a strong preference
and will use the first for now.
In order to declare the methods of these traits as being conditionally async,
the same `~async` syntax can actually be borrowed over from generic async functions -
`Self` will just be treated as another generic parameter with an `~async Trait` bound.
This produces a nice parallel between functions and traits,
as demonstrated below:

```rs
// What you write
~async fn f<T: ~async Trait>() { /* ... */ }

trait ~async Trait { ~async fn f(); }

// What it "expands" to
async fn f_async<T: async Trait>() { /* ... */ }
fn f_sync<T: Trait>() { /* ... */ }

trait async Trait { async fn f(); }
trait Trait { fn f(); }
```

And since those functions are actually just regular `~async` functions,
they also interact with generic parameters:

```rs
trait ~async Trait {
	~async fn f<T: ~async Read>(val: T);
}

// What it "expands" to
trait async Trait {
	async fn f_async<T: async Read>(val: T);
}
trait Trait {
	async fn f_async<T: async Read>(val: T);
	fn f_sync<T: Read>(val: T);
}

// A synchronous implementation
impl Trait for () {
	~async fn f<T: ~async Read>(val: T) {}
}
// An asynchronous implementation
impl async Trait for u32 {
	async fn f<T: async Read>(val: T) {}
}
// A generic implementation
impl<T: ~async Trait> ~async Trait for &T {
	~async fn f<T: ~async Read>(val: T) {}
}
```

Just like with regular `~async` functions,
the synchronous version only exists
when _all_ generic parameters
(here, both `T` and `Self`)
implement the trait synchronously.

The last thing to note is that
associated types in `~async Trait`s
would have the implicit bound `~async Drop`:
when the trait is an `async Trait`
they're allowed to be `async Drop`
but when it's a synchronous `Trait`
they are required to be `Drop`.
This should follow the rules that users will want most of the time.

To conclude,
I'll leave you with an annotated snippet
of how the `Iterator` trait might look with added `async` support:

```rs
pub trait ~async Iterator {
	type Item;

	~async fn next(&mut self) -> Option<Self::Item>;

	fn size_hint(&self) -> (usize, Option<usize>) {
		(0, None)
	}

	~async fn fold<B, F>(mut self, init: B, f: F) -> B
	where
		Self: Sized,
		// `fold` always drops `Self` at the end so this bound is required.
		Self: ~async Drop,
		F: ~async FnMut(B, Self::Item) -> B,
		// We can't relax B's bound because it's dropped in the event that
		// `self.next()` panics.
	{
		let mut accum = init;
		// `.await` is required in both cases because it could be a cancellation
		// point.
		while let Some(x) = self.next().await {
			accum = f(accum, x).await;
		}
		accum
	}

	fn map<B, F>(self, f: F) -> Map<Self, F>
	where
		Self: Sized,
		// Even a synchronous iterator's `map` accepts an `async FnMut` here,
		// without the tilde. This is because every `FnMut` is also an
		// `async FnMut`, so `async FnMut` is the strictly more general bound.
		// The tilde is only necessary when the function effectively needs to
		// specialize on the synchronous case to not be async, but that's not
		// necessary here since `map` isn't ever async anyway.
		F: async FnMut(Self::Item) -> B,
		// The default bounds are overly restrictive, so we relax them.
		F: ?Drop,
		B: ?Drop,
	{
		Map::new(self, f)
	}

	// et cetera
}
```

Compared to the current design of adding
a new `Stream`/`AsyncIterator` trait,
this has the following advantages:
- We don't have to decide between async vs sync callbacks
	for functions like `fold`
	(currently [futures-util][futures fold] and [tokio-stream][tokio fold] disagree about this).
- We don't have two separate functions
	`.map` and `.then` for sync and async respectively.
- `.map` with an async function can be called on a synchronous iterator,
	automatically turning it into an async one.
- There's no need for additional conversion functions
	like `.into_stream()` or `.into_async_iter()`.
- Existing iterators like `slice::Iter`
	will automatically implement the new `async Iterator` trait.

## Async traits and backwards compatibility { #async-traits-and-backwards-compatibility }

If you look closely at my definition of `Iterator` above
you'll notice that it's actually not backward compatible
with the current definition of `Iterator`.
The problem is that today,
people can override functions like `fold`
that are less powerful than the `~async` version.
For example:

```rs
impl Iterator for Example {
	type Item = ();

	fn next(&mut self) -> Option<Self::Item> { Some(()) }

	fn fold<B, F>(mut self, mut accum: B, f: F) -> B
	where
		F: FnMut(B, Self::Item) -> B,
	{
		loop { accum = f(accum, ()) }
	}
}
```

Under my definition of `Iterator`,
that code would instead need to be rewritten like this:

```rs
impl Iterator for Example {
	type Item = ();

	fn next(&mut self) -> Option<Self::Item> { Some(()) }

	~async fn fold<B, F>(mut self, mut accum: B, f: F) -> B
	where
		F: ~async FnMut(B, Self::Item) -> B,
	{
		loop { accum = f(accum, ()).await }
	}
}
```

The iterator itself is still not async,
but this change would additionally allow
calling `fold` with an asynchronous callback
even if the underlying iterator is still synchronous.

Unfortunately,
we can't just make the first version stop compiling
due to Rust's backward compatibility guarantees.
And even an edition won't be able to fix this,
since the issue is greater than just a syntactical one.

I don't think there is a reasonable way to somehow fix `fold` itself -
its signature is effectively set in stone at this point.
But we _can_ add a `[rs] where Self: Iterator<Item = Self::Item>` bound to it
and then have the generic version be under a new name,
`fold_async`.
Since `fold_async` would be strictly more general than `fold`,
the default implementation of `fold` can just forward to it.
So the definition of `Iterator` would actually look more like this:

```rs
pub trait ~async Iterator {
	type Item;

	~async fn next(&mut self) -> Option<Self::Item>;

	fn fold<B, F>(mut self, init: B, f: F) -> B
	where
		Self: Iterator<Item = Self::Item> + Sized + Drop,
		F: FnMut(B, Self::Item) -> B,
	{
		self.fold_async(init, f)
	}

	~async fn fold_async<B, F>(mut self, init: B, f: F) -> B
	where
		Self: Sized + ~async Drop,
		F: ~async FnMut(B, Self::Item) -> B,
	{
		let mut accum = init;
		while let Some(x) = self.next().await {
			accum = f(accum, x).await;
		}
		accum
	}

	// et cetera
}
```

Even though it looks very similar to not having async genericity at all,
it is still better than without
because:
1. Overriding `fold_async` also effectively overrides `fold` -
	they're able to share an implementation.
1. Async and sync iterators share definitions of `fold` and `fold_async`.

This makes the feature still worth it in my opinion,
even if we have to insert some hacks into `Iterator`
to avoid breaking compatibility.

Unfortunately
`fold` isn't the only method that would need this treatment,
potentially many others would too.
By my count, this includes (in the standard library alone):
`chain`,
`zip`,
`map`,
`for_each`,
`filter`,
`filter_map`,
`skip_while`,
`take_while`,
`map_while`,
`scan`,
`flat_map`,
`flatten`,
`inspect`,
`collect`,
`partition`,
`try_fold`,
`try_for_each`,
`reduce`,
`all`,
`any`,
`find`,
`find_map`,
`position`,
`rposition`,
`sum`,
`product`,
`cmp`,
`partial_cmp`,
`eq`,
`ne`,
`lt`,
`le`,
`gt`,
`ge`,
`DoubleEndedIterator::try_rfold`,
`DoubleEndedIterator::rfold`,
`DoubleEndedIterator::rfind` and
`Read::chain`.
If `async Clone` or `async Ord` become things,
the list would grow longer.

It is a bit of a shame that functions
like `map` and `Read::chain`
have to have async versions though,
since it's not like anyone overrides `map` anyway.
But because it's _technically_ possible,
Rust has already promised not to break that code
and so now can't relax the signature of that function.
Although who knows, maybe if we got a low % regression Crater run
it would convince people that's it's acceptable breakage
and the list could be shortened to the much more manageable
`for_each`,
`partition`,
`try_fold`,
`try_for_each`,
`reduce`,
`all`,
`any`,
`find`,
`find_map`,
`position`,
`rposition`,
`cmp`,
`partial_cmp`,
`eq`,
`ne`,
`lt`,
`le`,
`gt`,
`ge`,
`DoubleEndedIterator::try_rfold`,
`DoubleEndedIterator::rfold` and
`DoubleEndedIterator::rfind`.
I would definitely rather do this,
because frankly if you override `map`
then you deserve what you get.

Out of the group,
`collect`, `sum` and `product` are an especially interesting three
because their `_async` versions
(and their normal versions if we accept the technically breaking change)
can't use the standard `FromIterator`, `Product` and `Sum` traits
since those traits are currently hardcoded
to work for synchronous iterators only.
So we would instead have to make new `*Async` versions of those traits
with blanket implementations of the old versions:

```rs
// Not sure how useful `~async` is here; it would only be needed for collections
// that actually perform async work themselves while collecting as opposed to
// just potentially-asynchronously receiving the items and then synchronously
// collecting them.
//
// This is not true of any existing `FromIterator` or `FromStream`
// implementation currently, but there may still be use cases - who knows.
pub trait ~async FromAsyncIterator<A>: Sized {
    ~async fn from_async_iter<T: ~async IntoIterator<Item = A>>(iter: T) -> Self;
}
impl<T: FromAsyncIterator<A>, A> FromIterator<A> for T {
	fn from_iter<T: IntoIterator<Item = A>>(iter: T) -> Self {
		Self::from_async_iter(iter)
	}
}
```

With similar code for both `Sum` and `Product`.
Unlike `Iterator::fold`, since `from_iter`, `sum` and `product`
aren't default-implemented methods
we can't just add a new `from_async_iter` function
to the `FromIterator` trait itself;
an entirely new trait is needed.

## Trait impl implicit bounds { #trait-impl-implicit-bounds }

[Before](#function-implicit-bounds),
I talked about how inside an inherent impl block,
implicit `Drop` bounds to generics of the outer type
would apply individually to each of the methods depending on its asynchronity,
and the block itself would enforce no bounds on the type.
Unfortunately, we don't have that luxury when considering
trait implementations:
either the trait is implemented or it's not
and we can't apply our own bounds to individual items.

However,
we _do_ know whether the trait overall should be considered asynchronous or not -
whether it's being implemented as `async Trait` or `Trait`.
So we can just forward that property as the default kind of `Drop` bound,
and it should be what users want most of the time.
Of course, for the (hopefully) rare case that it's _not_ desired they can always override it.
The most obvious time that crops up is when implementing a trait that isn't an `async Trait`
but still has async methods (i.e. an async trait with no synchronous equivalent) -
then the drop bounds would end up overly restrictive:

```rs
trait ExampleTrait {
	async fn foo<V>(&self, value: V);
}

struct Wrapper<T>(T);

impl<T> ExampleTrait for Wrapper<T>
where
	// overly-restrictive implied bound: `T: Drop`
{
	async fn foo<V>(&self, value: V)
	where
		// implied bound: `V: async Drop` (since it's declared
		// on the function and not on the impl block)
	{
		todo!()
	}
}
```

But with any luck this kind of code won't be too common,
since users should ideally be writing most code as generic-over-async anyway.

An interesting side effect of the above rule is in code like below:

```rs
struct Wrapper<T>(T);

impl<T /* implied Drop bound */> Drop for Wrapper<T> {
	fn drop(&mut self) {
		println!("I am being dropped");
	}
}
```

Although it is not obvious, this code wouldn't compile
because the `Drop` implementation of a type
has more restrictive trait bounds than the type itself,
and that isn't allowed.
But since it looks like this code should compile,
I find it acceptable to introduce a special case and simply have the compiler
forward that implicit `T: Drop` bound to the type itself,
but only when a `Drop` implementation specifically is present.

Either way, that type does not work with `async Drop` types and the fix is like so:

```rs
struct Wrapper<T>(T);

impl<T: ?Drop> Drop for Wrapper<T> {
	fn drop(&mut self) {
		println!("I am being dropped");
	}
}
```

## Async closures { #async-closures }

Supporting async genericity with closures
(as required for functions like `Option::map` and `Iterator::fold`)
requires `async {Fn, FnMut, FnOnce}` to exist as traits.
It seems that this is a bit useless since we already have functions that return futures,
but as it turns out there is an actual benefit to having separate `async` function traits,
particularly when working with closures:
it makes the lifetimes a lot easier to manage,
since the returned futures will be able to borrow the closure and parameters -
something impossible with the current design.

However in order for the `async Fn`-traits to be useful,
they must be actually implemented by the relevant functions and closures.
Currently, people support asynchronous callbacks
by having closures that return futures (`|| async {}`) -
and `async fn`s are desugared to functions of this form too.
It wouldn't be a good idea to attempt to change the behaviour of the former
since that would need a hacky compiler special case for closures returning futures only,
but thankfully we have reserved a bit of syntax that would be perfect for this use case:
async closures (`async || {}`).
If they were to evaluate to closure types implementing `async Fn` instead of `Fn`,
they could be passed into async-generic functions like `Option::map` without a problem.

```rs
// Gives an `Option<T>`, since the async `map` is used.
let output = some_option.map(async |value| process(value).await).await;

// Gives an `Option<impl Future<Output = T>>`, since the sync `map` is used.
let output = some_option.map(|value| async { process(value).await });
```

The less good side of this addition is with `[rs] async fn`s:
we would have to choose between
keeping the current system of desugaring to
a simple `[rs] -> impl Future` function,
and implementing the `async Fn` traits.
The former is backwards compatible and more transparent
(since those functions can be replicated entirely in userspace),
but the latter has better interopability with async generic functions.
I am inclined to choose the latter design,
but it's an unfortunate decision to have to make.

Note that it wouldn't be possible to implement _both_ `async Fn` and `Fn`,
because implementing `Fn` already implies implementing `async Fn`
as an async function that never awaits;
we would end up with conflicting implementations of `async Fn`,
one that asynchronously evaluates to `T`
and one that immediately evaluates to `impl Future<Output = T>`.
To avoid that compile error we would have to choose one and discard the other.

## Conclusion { #conclusion }

In this post we sketched out a potential design for async drop,
figuring out many details and intricacies along the way.
The resulting proposal is unfortunately not a small one,
however it does have much general usefulness outside of async destructors
(`~async` in particular would be excellent to have
for so much code)
and lots of it is necessary if we are to minimize footguns.

As a summary of everything we've explored thus far:

1. We figured out the desired edge case semantics of async drop
	during cancellation, panics and assignments, in synchronous functions and with generics.
1. We explored a system for async destructors
	based on destructor futures instead of `poll_drop_ready`.
1. We explored a mechanism for supporting code
	that is generic over whether it is `async` or not.
1. We hypothesized what is best to apply as the default generic drop bounds in functions,
	as well as how to relax and strengthen them if necessary.
1. We considered how async genericity would impact functions and closures.

This post doesn't attempt to provide a final design for async drop -
there are still many open questions
(e.g. `UnwindSafe`, `?Drop` syntax, `[rs] #![no_std]` support)
and likely unknown unknowns.
But it does attempt to properly explore one particular design
to evaluate its complexity, feasability and usefulness.
Out of all possible options,
I think it to be quite a promising one
and definitely possible to implement in some form.

Many thanks to Yoshua Wuyts for proofreading this for me!

## Appendix A: Completion futures { #completion-futures }

Completion futures are a concept for a special type of future
that is guaranteed at compile-time to not be prematurely dropped or leaked,
in contrast to regular futures which can be stopped without warning at any time.
It doesn't sound like much,
but completion futures are actually incredibly useful:
- They enable `spawn` and `spawn_blocking` functions
	that don't restrict the future's lifetime to `'static`.
- They enable creating zero-cost wrappers around completion-based APIs
	like `io_uring`, IOCP and libusb.
- They enables better interopability with C++ futures,
	which have this guarantee by default.

I have previously written [a library for this](https://github.com/SabrinaJewson/completion)
but it was very limited because it fundamentally needed to rely on `unsafe`,
infecting just about every use of it with `unsafe` as well
which was really not ideal.
But it turns out
that with an async destructor design like the one proposed by this post,
it is much easier to support them
in an even more powerful way and with minimal `unsafe`.

The solution is to add a single new trait to the core library:

```rs
pub unsafe auto trait Leak {}
```

As an auto trait,
it would be implemented for every single type
other than a special `core::marker::PhantomNoLeak` marker
and any type transitively containing that.
What `Leak` represents is the ability to safely leak an instance of the type,
via [`mem::forget`], reference cycles or anything similar.
If a type opts out of implementing it,
it is guaranteed that from creation,
its `Drop` or `async Drop` implementation
will be run if the type's lifetime to end.

The standard library would have all the "leaky" APIs
like `Arc`, `Rc`, `ManuallyDrop` and `MaybeUninit`
require that `Leak` be implemented on the inner type,
to avoid safe code being able to circumvent the restriction.
Other than that, most other APIs would support both `Leak` and `!Leak` types,
since they will run the destructor of inner values.

And this is all we need to support completion futures.
An `io_uring` I/O operation future can be implemented by submitting the operation on creation
and waiting for it to complete on drop,
and the `!Leak` guarantee means that the [use-after-free issue](https://github.com/spacejam/rio/issues/30)
`io_uring` libraries currently have to work around is eliminated.

This is a very powerful feature,
even more so than my old `unsafe`-based implementation.
Because it guarantees not leaking from creation and not just from the first poll,
scoped tasks don't even need a special scope to be defined (à la [Crossbeam][crossbeam::scope]).
Instead, an API like this just works:

```rs
pub async fn spawn<'a, R, F>(f: F) -> JoinHandle<'a, R>
where
	F: Future<Output = R> + Send + 'a,
	R: Send,
{ /* ... */ }
```

It also has impacts on synchronous code,
because [`thread::spawn`] gets to be extended in a similar way:

```rs
pub fn spawn_scoped<'a, R, F>(f: F) -> JoinHandle<'a, R>
where
	F: FnOnce() -> R + Send + 'a,
	R: Send,
{ /* ... */ }
```

This would allow you to write code that borrows from the stack without problems:

```rs
let message = "Hello World".to_owned();

// Synchronous code
let thread_1 = thread::spawn_scoped(|| println!("{message}"));
let thread_2 = thread::spawn_scoped(|| println!("{message}"));
thread_1.join().unwrap();
thread_2.join().unwrap();

// Asynchronous code
let task_1 = task::spawn(async { println!("{message}") }).await;
let task_2 = task::spawn(async { println!("{message}") }).await;
task_1.await.unwrap();
task_2.await.unwrap();
```

Neat, right?

As with many things it needs an edition boundary to implement fully:
In the current edition, every generic parameter has to still imply `T: Leak`
but in future editions that can be relaxed to `T: ?Leak`,
allowing the small subset of APIs that _can_ leak values
(`Arc`, `Rc`, `mem::forget`, `ManuallyDrop`, etc)
to declare so in their signature
and the majority of APIs to have the less restrictive bound by default.

## Appendix B: Weakly async functions { #weakly-async-functions }

With the current design,
there ends up being a large number of functions
with the specific property
that they need to be `[rs] async fn`s
if a type they deal with is `async Drop`,
for the sole reason that they are able to panic
while they have that type in scope.
I listed a few at the start of the [async genericity](#async-genericity) section,
including `HashMap::{insert, entry}`, `Vec::push` and `Box::new`,
but there's one particularly relevant one here
which is `task::spawn`
(as seen in various runtimes:
[tokio](https://docs.rs/tokio/1/tokio/task/fn.spawn.html),
[async-std](https://docs.rs/async-std/1/async_std/task/fn.spawn.html),
[glommio](https://docs.rs/glommio/0.7/glommio/fn.spawn_local.html),
[smol](https://docs.rs/smol/1/smol/fn.spawn.html)).

Across all those runtimes,
`task::spawn` has the ability to panic before it spawns the future,
which commonly can happen if the runtime is not running,
but can also theoretically happen if allocation fails
or there's some other random system error.
The problem is that just because of this one small edge case
(and their presumed desire to support `async Drop` futures),
`task::spawn` is forced to be a full `async fn`
even though _in itself_ it doesn't do any `async` work.

This is especially bad for `task::spawn` as a function
because it can easily trip up those who are migrating code.
For example,
while before
this code would run the task
in parallel with `[rs] other_work()`:

```rs
let task = task::spawn(some_future);
other_work().await;
task.await;
```

With the changes applied
it would instead run `[rs] other_work()` and wait for it to complete,
and _then_ spawn the task and not even wait for it to finish!
(Unless of course dropping a task handle
would be changed to implicitly join the task,
which _may_ be a better design overall -
but the point still stands
because it doesn't run in parallel
as people would expect.)

The fixed version would look like this:

```rs
let task = task::spawn(some_future).await;
other_work().await;
task.await;
```

But given that the old version doesn't even fail to compile,
it's not an ideal situation to be in.
Additionally,
it does just look weird having a future that resolves to...another future.

My proposed solution to this problem
is to add a new type of function to the language called "weakly async functions"
which are in between asynchronous functions and synchronous functions.
Let's denote it here with ` [async] fn`, but the syntax is obviously up for bikeshedding.
The idea is this:

- ` [async] fn`s either complete synchronously or panic asynchronously.
- Because they must complete synchronously,
	they cannot be cancelled and thus they don't need to be `.await`ed - that can be made implicit.
- Because they panic asynchronously,
	they bypass the panic check
	and are allowed to own types with asynchronous destructors across potential panic points
	(but are not allowed to drop them unless via a panic).
- They are allowed to call regular `fn`s and other ` [async] fn`s, but not `async fn`s.
- They cannot be called from within synchronous functions.
- They are not allowed to recurse, just like `async fn`s.
- It is not a breaking change to convert from an ` [async] fn` to a regular fn.

This way,
`task::spawn`
(and a bunch of other functions
like `Box::new`, `Box::pin`, `Vec::push`, `Result::unwrap` etc)
would avoid requiring `.await`s when being called with `async Drop` types.
This solves the above footgun
while also contributing to the succintness of code.
`task::spawn` would be defined something like this:

```rs
pub [async] fn spawn<O, F>() -> JoinHandle<O>
where
	F: Future<Output = O> + Send + ?Drop + async Drop + 'static,
	O: Send,
```

And in asynchronous contexts would be callable with just `task::spawn(future)`,
no await necessary.

When inside generic code,
` [async]` would be treated as another state that `~async fn`s can be in,
meaning there are actually three ways to those functions.
There would additionally be `~[async] fn`s
for functions that can be either `fn`s or ` [async] fn`s,
but not `async fn`s.

You'd also need a special kind of bound to represent
"`Drop` when the function is synchronous
and `async Drop` when the function is `async`,
but also `async Drop` when the function is ` [async]`,
since this function does not drop a value of this type
unless it panics".
For now I will use the incredibly verbose form `~[async] async Drop`
to represent this,
but if this feature is actually added
a better and more bikeshedded syntax will probably have to be chosen.

This is the feature that allows us to define `Vec::push` generically:

```rs
impl<T> Vec<T> {
	~[async] fn push(&mut self, item: T)
	where
		T: ?Drop + ~[async] async Drop,
	{
		/* ... */
	}
}

// "Expanded" version
impl<T> Vec<T> {
	fn push_sync(&mut self, item: T)
	where
		T: Drop,
	{
		/* ... */
	}
	~[async] fn push_weak_async(&mut self, item: T)
	where
		T: ?Drop + async Drop,
	{
		/* ... */
	}
}
```

Remember that this function can drop `item`
and so can't be fully synchronous,
but also doesn't drop `item` unless it's panicking
and so shouldn't be made fully `async` either.
As such it uses the in-between,
supporting `async Drop` (and therefore also ` [async] Drop`) when it is an ` [async] fn`
and `Drop` when it is a `fn`.

Unlike completion futures,
I'm not so certain whether this is a good idea or not,
or whether there aren't any other simpler alternatives.
But I do definitely think
there is a problem here that does need to be addressed somehow,
and to me this seems the best way to do it.

## Appendix C: Linear types { #linear-types }

I feel that I have to mention linear types at least once,
given how much discourse there has been about them.
A linear type is defined as "a type that must be used exactly once".
It turns out this definition is slightly vague,
because it can refer to two things:

1. Types which do not have any kind of `Drop` implementation and must be handled explicitly,
	but can be leaked with functions like [`mem::forget`].
1. Types which do have destructors and so can implicitly fall out of scope,
	but can't be leaked with functions like [`mem::forget`]
	(so they are guaranteed to be able to run code before falling out of scope).

The former is a more common definition of linear types,
and allows for types to force their users to be more explicit
about what happens to them when they're destroyed.
I don't have a proposal for this,
but simply by coincidence the proposed `?Drop` bound feature
does orient itself towards supporting linear types of this sort in future
and although personally I do not think they will be worth adding,
their viability has been increased as a side-effect.

The latter definition is what is implemented by the above [completion futures](#completion-futures) proposal.
In a way it's not true linear types,
but it's the only one that gives the practical benefits of things like
zero-cost `io_uring` and scoped tasks.
It is also a lot less difficult to integrate into existing Rust code,
which tends to rely quite heavily on destructors existing
but not so much on values being safely leakable.

## Appendix D: Uncancellable futures { #uncancellable-futures }

I previously argued against [Carl Lerche's suggestion to make all async functions uncancellable][6 ways]
in favour of defining consistent semantics for `.await` rather than removing it.
However, these kinds of functions not totally off the table;
such a feature can still definitely exist,
first of all as a userspace combinator:

```rs
pub async fn must_complete<F: Future>(fut: F) -> F::Output {
	MustComplete(fut).await
}

#[pin_project(PinnedDrop)]
struct MustComplete<F: Future>(#[pin] F);

impl<F: Future + ?Drop + async Drop> Future for MustComplete<F> {
	type Output = F::Output;

	fn poll(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Self::Output> {
		self.project().0.poll(cx)
	}
}

#[pinned_drop]
impl<F: Future> async PinnedDrop for MustComplete<F> {
	async fn drop(self: Pin<&mut Self>) {
		self.project().0.await;
	}
}
```

Usable like so:

```rs
must_complete(async {
	some_very_important_work().await;
	that_must_not_be_interrupted().await;
})
.await;
```

It could also exist as a language feature,
which would additionally allow removing `.await` if that is desired.
Either way, the effect is the same:
this proposal easily enables writing futures that are guaranteed to not have cancellation points.
Personally I do not think this use case is common enough to warrant a language feature,
but it is still definitely worth considering.

[6 ways]: https://carllerche.netlify.app/2021/06/17/six-ways-to-make-async-rust-easier/
[Futures Concurrency III]: https://blog.yoshuawuyts.com/futures-concurrency-3/
[`CancellationToken`]: https://docs.rs/tokio-util/0.7/tokio_util/sync/struct.CancellationToken.html
[`Option::insert`]: https://doc.rust-lang.org/stable/std/option/enum.Option.html#method.insert
[`RefUnwindSafe`]: https://doc.rust-lang.org/stable/std/panic/trait.RefUnwindSafe.html
[`StopToken`]: https://docs.rs/stop-token/0.7/stop_token/struct.StopToken.html
[`UnwindSafe`]: https://doc.rust-lang.org/stable/std/panic/trait.UnwindSafe.html
[`mem::forget`]: https://doc.rust-lang.org/stable/std/mem/fn.forget.html
[`needs_drop`]: https://doc.rust-lang.org/stable/std/mem/fn.needs_drop.html
[`poll_close`]: https://docs.rs/futures-io/0.3/futures_io/trait.AsyncWrite.html#tymethod.poll_close
[`poll_shutdown`]: https://docs.rs/tokio/1/tokio/io/trait.AsyncWrite.html#tymethod.poll_shutdown
[`std::thread::panicking`]: https://doc.rust-lang.org/stable/std/thread/fn.panicking.html
[`thread::spawn`]: https://doc.rust-lang.org/stable/std/thread/fn.spawn.html
[crossbeam::scope]: https://docs.rs/crossbeam/0.8/crossbeam/fn.scope.html
[oom=panic]: https://github.com/rust-lang/rust/issues/43596
[futures fold]: https://docs.rs/futures-util/0.3/futures_util/stream/trait.StreamExt.html#method.fold
[tokio fold]: https://docs.rs/tokio-stream/0.1/tokio_stream/trait.StreamExt.html#method.fold
[tcp nonblocking]: https://doc.rust-lang.org/stable/std/net/struct.TcpStream.html#method.set_nonblocking
