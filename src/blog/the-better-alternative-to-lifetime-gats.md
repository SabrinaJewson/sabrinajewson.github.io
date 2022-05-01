{
	"published": "2022-05-01"
}

# The Better Alternative to Lifetime GATs

## Where real GATs fall short { #where-real-gats-fall-short }

[GATs] are an unstable feature of Rust,
likely to be stabilized in the next few versions,
that allow you to add generic parameters
on associated types in traits.
The motivating example for this feature is the "lending iterator" trait,
which allows you to define an iterator
for which only one of its items can exist at any given time.
With lifetime GATs,
its signature would look something like this:

```rs
pub trait LendingIterator {
	type Item<'this>
	where
		Self: 'this;
	fn next(&mut self) -> Option<Self::Item<'_>>;
}
```

and it would allow you to implement iterators
you otherwise wouldn't have been able to,
like `WindowsMut`
(since the slices it returns overlap,
a regular iterator won't work):

```rs
use ::core::mem;

pub fn windows_mut<T, const WINDOW_SIZE: usize>(
	slice: &mut [T],
) -> WindowsMut<'_, T, WINDOW_SIZE> {
	assert_ne!(WINDOW_SIZE, 0);
	WindowsMut { slice, first: true }
}

pub struct WindowsMut<'a, T, const WINDOW_SIZE: usize> {
	slice: &'a mut [T],
	first: bool,
}

impl<'a, T, const WINDOW_SIZE: usize> LendingIterator
	for WindowsMut<'a, T, WINDOW_SIZE>
{
	type Item<'this> = &'this mut [T; WINDOW_SIZE] where 'a: 'this;

	fn next(&mut self) -> Option<Self::Item<'_>> {
		if !self.first {
			self.slice = &mut mem::take(&mut self.slice)[1..];
		}
		self.first = false;

		Some(self.slice.get_mut(..WINDOW_SIZE)?.try_into().unwrap())
	}
}
```

Great!
That's our `LendingIterator` trait,
done and dusted,
and we've proven that it works.
End of article.

Well, before we go let's just try one last thing:
actually consuming the `WindowsMut` iterator.
There's no need to really because I'm sure it'll work,
but we'll do it anyway for the learning experience, right?

So first we'll define a function
that prints each element of a lending iterator.
This is pretty simple,
we just have to use [HRTBs] to write the trait bound
and a `[rs] while let` loop for the actual consumption.

```rs
fn print_items<I>(mut iter: I)
where
	I: LendingIterator,
	for<'a> I::Item<'a>: Debug,
{
	while let Some(item) = iter.next() {
		println!("{item:?}");
	}
}
```

All good so far, this compiles fine.
Now we'll actually call it with an iterator:

```rs
print_items::<WindowsMut<'_, _, 2>>(windows_mut(&mut [1, 2, 3]));
```

This should obviously compile since `[rs] &mut [i32; 2]` is definitely `Debug`.
So we can just run `cargo run` and see the ou–

```
error[E0716]: temporary value dropped while borrowed
  --> src/main.rs:45:58
   |
45 |     print_items::<WindowsMut<'_, _, 2>>(windows_mut(&mut [1, 2, 3]));
   |     -----------------------------------------------------^^^^^^^^^--
   |     |                                                    |
   |     |                                                    creates a temporary which is freed while still in use
   |     argument requires that borrow lasts for `'static`
46 | }
   | - temporary value is freed at the end of this statement
```

oh.

oh no.

## What went wrong? { #what-went-wrong }

Clearly, something's not right here.
rustc is telling us that for some reason,
our borrow of the array `[rs] [1, 2, 3]` is required to live for `[rs] 'static` —
but we haven't written any `[rs] 'static` bounds anywhere,
so this doesn't really make much sense.
We'll have to put ourselves in the mindset of the compiler for a bit
so that we can try to figure out what's happening.

First of all,
we create an iterator of `[rs] WindowsMut<'0, i32, 2>`,
where `'0` is the name of some local lifetime
(notably, this lifetime is necessarily shorter than `[rs] 'static`).
Then we pass this iterator type into the function `print_items`,
in doing so
setting its `I` generic parameter to the aforementioned type `[rs] WindowsMut<'0, i32, 2>`.

So now we just need to make sure that the trait bounds hold.
Substituting `I` for its actual type
in the `[rs] where` clause of `print_items`,
we get this bound that needs to be checked:

```rs
where
	for<'a> <WindowsMut<'0, i32, 2> as LendingIterator>::Item<'a>: Debug,
```

The `[rs] for<'a>` syntax means that
we must verify that _any_ lifetime can be substituted
in the right hand side
and the trait bound must still pass.
A good edge case to check here is `[rs] 'static`,
since we know that if that check fails
the overall bound will definitely fail.
So we end up with this:

```rs
where
	<WindowsMut<'0, i32, 2> as LendingIterator>::Item<'static>: Debug,
```

Or in other words,
the associated item type of `WindowsMut`
must implement `Debug`
when fed the lifetime `[rs] 'static`.
Let's hop back to the implementation of `LendingIterator` for `WindowsMut`
to see if that actually holds.
As a quick refresher,
the relevant bit of code is here:

```rs
impl<'a, T, const WINDOW_SIZE: usize> LendingIterator
	for WindowsMut<'a, T, WINDOW_SIZE>
{
	type Item<'this> = &'this mut [T; WINDOW_SIZE] where 'a: 'this;
	/* ... */
}
```

Uhh…that's a bit complex.
Let's replace the generic types with our concrete ones to simplify it.

```rs
impl LendingIterator for WindowsMut<'0, i32, 2> {
	type Item<'static> = &'static mut [i32; 2]
	where
		'0: 'static;
}
```

And now we can finally see what's going wrong.
As we established earlier,
`'0` is the local lifetime of `[rs] [1, 2, 3]`
and is therefore definitely a shorter lifetime than `[rs] 'static`.
This means that there is absolutely no way that the bound `[rs] '0: 'static` will hold,
making `[rs] <WindowsMut<'0, i32, 2> as LendingIterator>::Item<'static>` an invalid type altogether.
So of course the compiler can't verify
that it implements `Debug` —
it doesn't even exist at all!
This was what the compiler was really trying to tell us earlier,
even if it was a bit obtuse about it.

The ultimate conclusion of all this
is that HRTBs basically can't be used with lifetime GATs at all.
`[rs] for<'a>` just doesn't express the right requirement —
we don't want to require the bound for _any_ lifetime,
we only really want to require it for lifetimes _shorter than `'0`_.
Ideally, we would be able to write in a `[rs] where` clause there,
so the bounds of `print_items` could become:

```rs
fn print_items<I>(mut iter: I)
where
	I: LendingIterator,
	for<'a where I: 'a> I::Item<'a>: Debug,
```

This would mean that `[rs] 'static` can't be selected as the lifetime chosen for the HRTB
since `[rs] WindowsMut<'0, i32, 2>` is _definitely not_ `[rs] 'static`,
so our above proof-by-contradiction would no longer work
and the compiler would accept our correct code without problem.

But unfortunately it doesn't look like we'll be getting this feature any time soon.
At the time of writing
I do not know of
any RFC or formal suggestion for this feature
(other than [one rust-lang/rust issue][where hrtb issue])
so it'll be a long time before it actually arrives on stable
should we get it at all.
Until then,
we're stuck with a hard limitation every time you use lifetime GATs:
you can't place trait bounds on GATs or require them to be a specific type
unless the trait implementor is `[rs] 'static`.

This makes real GATs practically unusable for most use cases.
I'm still happy they're being stabilized,
but they likely won't see wide adoption in APIs
until this problem is solved.

So, what can we do in the meantime?

## Workaround 1: `dyn Trait` as a HKT { #dyn-trait-as-a-hkt }

As first shared in [this gist] by [@jix],
one workaround is to use `dyn Trait` as a form of HKT,
because `dyn Trait` accepts an HRTB in its type,
_and_ supports changing associated types based on the HRTB's lifetime.

To implement the design in our code,
first we modify the `LendingIterator` trait to look like this:

```rs
pub trait GivesItem<'a> {
	type Item;
}

pub trait LendingIterator {
	type Item: ?Sized + for<'this> GivesItem<'this>;
	fn next(&mut self) -> Option<<Self::Item as GivesItem<'_>>::Item>;
}
```

The magic comes in the implementation of `LendingIterator` for specific types.
For `WindowsMut` it looks like this:

```rs
impl<'a, T, const WINDOW_SIZE: usize> LendingIterator
	for WindowsMut<'a, T, WINDOW_SIZE>
{
	type Item = dyn for<'this> GivesItem<
		'this,
		Item = &'this mut [T; WINDOW_SIZE],
	>;

	/* ... */
}
```

As you can see,
the `Item` type is set to a `dyn Trait` with an HRTB,
where the `dyn Trait`'s associated type depends on the input HRTB lifetime.
So even though `[rs] type Item` is only a single type,
it actually acts like a function from a lifetime to a type,
just like a real GAT.

We can then modify the signature of `print_items` like so:

```rs
fn print_items<I>(mut iter: I)
where
	I: LendingIterator,
	for<'a> <I::Item as GivesItem<'a>>::Item: Debug,
```

And lo and behold, it works!

```rs
[1, 2]
[2, 3]
```

However,
this approach runs into some nasty limitations
rather quickly.
Let's say that we have now defined a mapping operation on lending iterators:

```rs
pub fn map<I, F>(iter: I, mapper: F) -> Map<I, F>
where
	I: LendingIterator,
	F: for<'a> Mapper<'a, <I::Item as GivesItem<'a>>::Item>,
{
	Map { iter, mapper }
}

pub struct Map<I, F> {
	iter: I,
	mapper: F,
}

impl<I, F> LendingIterator for Map<I, F>
where
	I: LendingIterator,
	F: for<'a> Mapper<'a, <I::Item as GivesItem<'a>>::Item>,
{
	type Item = dyn for<'this> GivesItem<
		'this,
		Item = <F as Mapper<'this, <I::Item as GivesItem<'this>>::Item>>::Output,
	>;

	fn next(&mut self) -> Option<<Self::Item as GivesItem<'_>>::Item> {
		self.iter.next().map(&mut self.mapper)
	}
}

// Trait helper to allow the lifetime of a mapping function's output to depend
// on its input. Without this, `map` on an iterator would always force lending
// iterators to become non-lending which we don't really want.
pub trait Mapper<'a, I>: FnMut(I) -> <Self as Mapper<'a, I>>::Output {
	type Output;
}

impl<'a, I, F, O> Mapper<'a, I> for F
where
	F: FnMut(I) -> O,
{
	type Output = O;
}
```

and then decide to use a mapped iterator instead of the normal one:

```rs
let mut array = [1, 2, 3];
let iter = windows_mut::<_, 2>(&mut array);

fn mapper(input: &mut [i32; 2]) -> &mut i32 {
	&mut input[0]
}
let mapped = map(iter, mapper);

print_items::<Map<_, _>>(mapped);
```

This works fine, printing the desired result of `1` followed by `2`.

But
if we suddenly decide that the code in `print_items` should be inlined,
we're in for a not-so-fun little surprise:

```rs
let mut mapped = map(iter, mapper);

while let Some(item) = mapped.next() {
	println!("{item:?}");
}
```

```
error[E0308]: mismatched types
  --> src/main.rs:97:35
   |
97 |     while let Some(item) = mapped.next() {
   |                                   ^^^^ one type is more general than the other
   |
   = note: expected associated type `<(dyn for<'this> GivesItem<'this, for<'this> Item = &'this mut [i32; 2]> + 'static) as GivesItem<'_>>::Item`
              found associated type `<(dyn for<'this> GivesItem<'this, for<'this> Item = &'this mut [i32; 2]> + 'static) as GivesItem<'this>>::Item`
```

To be honest,
I have absolutely no idea what this error message is saying —
but I'm pretty sure it's just nonsense
because the generic version works fine.

This isn't the worst problem in the world —
it's inconvenient but it can probably always be worked around.
That said, it is still possible to improve the ergonomics.

## Workaround 2: HRTB supertrait { #hrtb-supertrait }

Let's try a different approach then.
We'll start again from the real GAT version,
but this time with explicit lifetimes
(you'll see why in a minute):

```rs
pub trait LendingIterator {
	type Item<'this> where Self: 'this;
	fn next<'this>(&'this mut self) -> Option<Self::Item<'this>>;
}
```

You'll notice that all items of the trait use the `[rs] 'this` lifetime.
So we can eliminate the use of GATs by raising that lifetime up one level,
to become a generic parameter of the whole trait
instead of each item on the trait.

```rs
pub trait LendingIterator<'this>
// This where bound is raised from the GAT
where
	Self: 'this,
{
	type Item;
	fn next(&'this mut self) -> Option<Self::Item<'this>>;
}
```

This way, `[rs] for<'a> LendingIterator<'a>` becomes
an identical trait to the old `LendingIterator` trait —
given a specific lifetime, we get both a `next` function and `Item` associated type.

However, there are a few problems with a trait declared this way:
1. `[rs] fn next(&'this mut self)` is verbose and doesn't allow eliding the lifetimes.
1. The trait bound `[rs] for<'a> LendingIterator<'a>` is long and inconvenient to spell out.
1. Some functions like `for_each` need `Self` to implement `[rs] for<'a> LendingIterator<'a>`
	in order for their signature to work.
	But
	it's hard to express that within a trait `[rs] LendingIterator<'this>`
	where the HRTB is not already present.

To solve them we can split the trait into two,
moving the parts that can have generic parameters (functions)
into an outer lifetime-less subtrait
and the parts that can't have generic parameters (types)
into an inner lifetimed supertrait:

```rs
pub trait LendingIteratorLifetime<'this>
where
	Self: 'this,
{
	type Item;
}

pub trait LendingIterator: for<'this> LendingIteratorLifetime<'this> {
	fn next(&mut self) -> Option<<Self as LendingIteratorLifetime<'_>>::Item>;
}
```

Now we can finally get to reimplementing `WindowsMut`:

```rs
impl<'this, 'a, T, const WINDOW_SIZE: usize> LendingIteratorLifetime<'this>
	for WindowsMut<'a, T, WINDOW_SIZE>
where
	Self: 'this,
{
	type Item = &'this mut [T; WINDOW_SIZE];
}

impl<'a, T, const WINDOW_SIZE: usize> LendingIterator
	for WindowsMut<'a, T, WINDOW_SIZE>
{
	fn next(&mut self) -> Option<<Self as LendingIteratorLifetime<'_>>::Item> {
		if !self.first {
			self.slice = &mut mem::take(&mut self.slice)[1..];
		}
		self.first = false;

		Some(self.slice.get_mut(..WINDOW_SIZE)?.try_into().unwrap())
	}
}
```

Let's try it out then!
Just run `cargo build` and…

```
error[E0477]: the type `WindowsMut<'a, T, WINDOW_SIZE>` does not fulfill the required lifetime
  --> src/main.rs:41:39
   |
41 | impl<'a, T, const WINDOW_SIZE: usize> LendingIterator
   |                                       ^^^^^^^^^^^^^^^
```

Right — I should know better
than to expect things to work first try
at this point.

That error's extremely unhelpful,
but there is actually a legitimate explanation for what's happening here.
Once again putting on our compiler hats,
one of our jobs when checking a trait implementation
is to check whether the supertraits hold.
In this case that means we have to satisfy this trait bound:

```rs
WindowsMut<'a, T, WINDOW_SIZE>: for<'this> LendingIteratorLifetime<'this>
```

Like before,
a good edge case to check for with HRTB bounds
is whether substituting in `[rs] 'static` holds.
In other words,
a necessary condition for the above bound to be satisfied
is that this bound is also satisfied:

```rs
WindowsMut<'a, T, WINDOW_SIZE>: LendingIteratorLifetime<'static>
```

So let's check that.
Jumping to the implementation of `LendingIteratorLifetime` for `WindowsMut`,
we see this:

```rs
impl<'this, 'a, T, const WINDOW_SIZE: usize> LendingIteratorLifetime<'this>
	for WindowsMut<'a, T, WINDOW_SIZE>
where
	Self: 'this,
```

and substituting in `[rs] 'this` for `[rs] 'static`:

```rs
impl<'a, T, const WINDOW_SIZE: usize> LendingIteratorLifetime<'static>
	for WindowsMut<'a, T, WINDOW_SIZE>
where
	Self: 'static,
```

...ah.
`[rs] Self: 'static`.
That's probably a problem.

Indeed,
if we add a `[rs] where Self: 'static` to the `LendingIterator` implementation
it _does_ compile:

```rs
impl<'a, T, const WINDOW_SIZE: usize> LendingIterator
	for WindowsMut<'a, T, WINDOW_SIZE>
where
	Self: 'static,
```

But that's definitely not something we want to do —
it would mean that `WindowsMut`
would only work on
empty slices, global variables and leaked variables.

This is a very similar problem
to the one we faced before with the GAT version:
ideally, we'd be able to specify a `[rs] where` clause
within the `[rs] for<'a>` bound
so that only lifetimes shorter than `Self` could be substituted in,
excluding lifetimes like `[rs] 'static` for non-`[rs] 'static` `Self`s.
The signature could look something like this:

```rs
pub trait LendingIterator
where
	Self: for<'this where Self: 'this> LendingIteratorLifetime<'this>,
```

But just as before
`[rs] where` clauses in HRTBs unfortunately don't exist yet,
so it looks like this is just another dead end.
What a shame.

## HRTB implicit bounds { #hrtb-implicit-bounds }

> Having failed thoroughly in your mission
> to bring reliable and stable lifetime GATs to the Rust ecosystem,
> you quit programming altogether out of shame
> and vow to live out the rest of your days
> as a lowly potato farmer
> in the countryside.
> With nothing but a small amount savings and a dream,
> you move in to a run-down stone farmhouse
> in Scotland
> where you can live onwards peacefully and undisturbed.
>
> Many years pass.
> You have grown accustomed to nature:
> you have seen plants grow, wither and die before your eyes
> more times than smallvec has had CVEs,
> and the seasons are now no more than a blur —
> day, night, summer, winter all morphing into one another
> and passing faster than the blink of an eye.
> You sleep deeply and peacefully every night,
> safe and comfortable in the knowledge
> that you'll never have to deal with
> wall of text linker errors ever again.
> You have become so familiar with the pathways and routes around your home
> that you can walk them in your sleep.
> Every single nook and cranny of the place
> down to the most minute detail
> is etched deep into your brain:
> the position of each plant,
> the location of every nest,
> the size and shape of each pebble.
>
> So it is no surprise that on one chilly March morning,
> you immediately notice the abnormal presence
> of a thin white object sticking out from under a bush.
> Drawing closer, it appears to be a piece of paper,
> slightly damp from absorbing the cold morning dew.
> You pick it up,
> and as you stare at the mysterious sigils printed on the page,
> slowly — very slowly — a vague memory begins to come back to you.
> That's right,
> it's "Rust".
> And this "Rust" on the page appears to form
> a very short program:
> ```rs
> fn example<T>(value: T)
> where
> 	for<'a> &'a T: Debug,
> {
> 	eprintln!("{:?}", &value);
> }
> let array = [1, 2, 3];
> example(&array);
> ```
>
> As you make your way back to the farmhouse,
> mysterious piece of paper in hand,
> you ponder about what it could mean.
> Of course, there's no way it would compile,
> you know _that_ much:
> `[rs] for<'a>` would be able to select `[rs] 'static` as its lifetime,
> meaning `[rs] &'static T` would need to implement `Debug`,
> which is obviously not true for the `[rs] &'array [i32; 3]` shown
> (as `[rs] &'static &'array [i32;  3]` can't even exist,
> let alone be `Debug`).
>
> So why would someone go to the effort of printing out code that doesn't even work —
> and what's more, placing it all the way in your farm?
> It is this that you wonder about
> while you dig out your old laptop
> from deep inside storage.
> It hasn't been touched for five years,
> so it's gotten a little dusty —
> but you press the power button
> and screen bursts into colour and life,
> exactly as it used to do those so many years ago.
>
> Tentatively,
> you open a text editor,
> and begin copying out the contents of that paper
> inside it.
> Now, how do I build it again?
> Shipment? Freight? Haul?
> No, it was something different…ah, cargo, that was it.
> Into the shell you type out the words you haven't seen for so, so long:
> ```sh
> cargo run
> ```
> You take a deep breath,
> and then press the enter key.
> The fan whirrs as the CPU starts into life.
> For a short moment that feels like an eon,
> Cargo displays "Building" —
> but eventually it finishes,
> and as it does,
> one line of text rolls down the screen:
> ```rs
> [1, 2, 3]
> ```

Wait, what?
Do that again.

> You take a deep breath,
> and then press the enter key.
> The fan whirrs as the CPU starts into life.
> For a short moment that feels like an eon,
> Cargo displays "Building" —
> but eventually it finishes,
> and as it does,
> one line of text rolls down the screen:
> ```rs
> [1, 2, 3]
> ```

So it wasn't just a fluke.
But that makes no sense at all:
by all the rules we knew,
there is _no way_ that code should've compiled.
So what's happening here?

The answer is that
while `[rs] for<'a>` does not support explicit `[rs] where` clauses,
it actually can, sometimes,
have an _implied_ `[rs] where` clause —
in this case, it's `[rs] for<'a where I: 'a>`.
But it only occurs in specific scenarios:
in particular,
when there is an _implicit bound_ in the type or trait bound
the HRTB is applied to,
that implicit bound gets forwarded to the implicit `[rs] where` clause of the HRTB.

An implicit bound is a trait bound that is present,
but not stated explicitly
by a colon in the generics or `[rs] where` clause.
As you can infer from the example above,
`[rs] &'a T` contains an implicit bound for `[rs] T: 'a` —
this is a really simple rule to prevent nonsense types like `[rs] &'static &'short_lifetime i32`
(a reference that outlives borrowed contents).
It's this rule
that causes `[rs] for<'a> &'a T`
to act like it's actually `[rs] for<'a where T: 'a> &'a T`,
enabling that code to run
and successfully print `[rs] [1, 2, 3]`.

Implicit bounds can appear on structs too.
For example, take this struct:

```rs
#[derive(Debug)]
struct Reference<'a, T>(&'a T);
```

Because `[rs] &'a T` has an implicit bound of `[rs] T: 'a`,
the struct `Reference` _also_ has an implicit bound of `[rs] T: 'a`.
You can prove this because this code compiles:

```rs
fn example<T>(value: T)
where
	for<'a /* where T: 'a */> Reference<'a, T>: Debug,
{
	dbg!(Reference(&value));
}

let array = [1, 2, 3];
example(&array);
```

However,
as soon as you try to upgrade the implicit bound to an explicit one
you will notice it no longer compiles:

```rs
#[derive(Debug)]
struct Reference<'a, T: 'a>(&'a T);

fn example<T>(value: T)
where
	for<'a> Reference<'a, T>: Debug,
{
	dbg!(Reference(&value));
}

let array = [1, 2, 3];
example(&array);
```

```
error[E0597]: `array` does not live long enough
  --> src/main.rs:15:13
   |
15 |     example(&array);
   |     --------^^^^^^-
   |     |       |
   |     |       borrowed value does not live long enough
   |     argument requires that `array` is borrowed for `'static`
16 | }
   | - `array` dropped here while still borrowed
```

Implicit bounds in HRTBs are…a very weird feature of Rust.
I'm still not sure whether they are intended to exist
or are just an obscure side-effect of the current implementation.
But either way,
this is an incredibly useful feature for us.
If we can somehow leverage this to apply it in our supertrait HRTB of `LendingIterator`,
then we can maybe get it to actually work without the `[rs] 'static` bound!
Thanks, mysterious piece of paper.

## Workaround 3: The better GATs { #the-better-gats }

Armed with our new knowledge of implied bounds,
all we have to do is get it
to work in conjuction with that `[rs] for<'a> LendingIteratorLifetime<'a>` supertrait.
One way to achieve this is to
introduce a new dummy type parameter to `LendingIteratorLifetime`,
so that HRTBs can make use of it to apply their own implicit bounds:

```rs
pub trait LendingIteratorLifetime<'this, ExtraParam> {
	type Item;
}

pub trait LendingIterator
where
	Self: for<'this /* where Self: 'this */>
		LendingIteratorLifetime<'this, &'this Self>,
{
	fn next(&mut self) -> Option<<Self as LendingIteratorLifetime<'_, &Self>>::Item>;
}
```

This _works_, but it's a pain to have to write out `[rs] &'this Self`
every time you want to use the trait.
Ergonomics can be improved slightly by using a default type parameter:

```rs
// Give every usage of this trait an implicit `where Self: 'this` bound
pub trait LendingIteratorLifetime<'this, ImplicitBounds = &'this Self> {
	type Item;
}

pub trait LendingIterator
where
	Self: for<'this /* where Self: 'this */> LendingIteratorLifetime<'this>,
{
	fn next(&mut self) -> Option<<Self as LendingIteratorLifetime<'_>>::Item>;
}
```

There is still one slight improvement we can make
to reduce the chance the API is accidentally misused
by setting the `ImplicitBounds` parameter
to something other than `[rs] &'this Self`,
and that is using a sealed type and trait.
This leads to my current recommended definition for this trait:

```rs
pub trait LendingIteratorLifetime<'this, ImplicitBounds: Sealed = Bounds<&'this Self>> {
	type Item;
}

mod sealed {
	pub trait Sealed: Sized {}
	pub struct Bounds<T>(T);
	impl<T> Sealed for Bounds<T> {}
}
use sealed::{Bounds, Sealed};

pub trait LendingIterator: for<'this> LendingIteratorLifetime<'this> {
	fn next(&mut self) -> Option<<Self as LendingIteratorLifetime<'_>>::Item>;
}
```

New trait in hand, we can rewrite our type `WindowsMut` to use it:

```rs
impl<'this, 'a, T, const WINDOW_SIZE: usize> LendingIteratorLifetime<'this>
	for WindowsMut<'a, T, WINDOW_SIZE>
{
	type Item = &'this mut [T; WINDOW_SIZE];
}

impl<'a, T, const WINDOW_SIZE: usize> LendingIterator
	for WindowsMut<'a, T, WINDOW_SIZE>
{
	fn next(&mut self) -> Option<<Self as LendingIteratorLifetime<'_>>::Item> {
		if !self.first {
			self.slice = &mut mem::take(&mut self.slice)[1..];
		}
		self.first = false;

		Some(self.slice.get_mut(..WINDOW_SIZE)?.try_into().unwrap())
	}
}
```

as well as `Map` (the `Mapper` trait is still needed):

```rs
impl<'this, I, F> LendingIteratorLifetime<'this> for Map<I, F>
where
	I: LendingIterator,
	F: for<'a> Mapper<'a, <I as LendingIteratorLifetime<'a>>::Item>,
{
	type Item = <F as Mapper<
		'this,
		<I as LendingIteratorLifetime<'this>>::Item,
	>>::Output;
}

impl<I, F> LendingIterator for Map<I, F>
where
	I: LendingIterator,
	F: for<'a> Mapper<'a, <I as LendingIteratorLifetime<'a>>::Item>,
{
	fn next(&mut self) -> Option<<Self as LendingIteratorLifetime<'_>>::Item> {
		self.iter.next().map(&mut self.mapper)
	}
}
```

and unlike both real GATs and [workaround 1],
this works with both consuming the concrete type directly
_and_ through the generic `print_items` function.
Perfect!

## Dyn safety { #dyn-safety }

The main disadvantage of workaround 3 in comparison to workaround 1
is that it is not `dyn`-safe.
If you try to use it as a trait object,
`rustc` helpfully tells you this:

```
note: for a trait to be "object safe" it needs to allow building a vtable to allow the call to be resolvable dynamically; for more information visit <https://doc.rust-lang.org/reference/items/traits.html#object-safety>
   --> src/main.rs:14:28
    |
14  | pub trait LendingIterator: for<'this> LendingIteratorLifetime<'this> {
    |           ---------------  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ ...because it uses `Self` as a type parameter
    |           |
    |           this trait cannot be made into an object...
```

When it says "because it uses `Self` as a type parameter"
it's actually referring to the hidden `[rs] Bounds<&'this Self>` default parameter
we inserted.
As a result, making `LendingIterator` _directly_ work with `dyn` is simply not possible.

But that is _not_ to say that dynamic dispatch is altogether impossible —
all we have to do is define a helper trait for it!
And as long as that helper trait uses workaround 1,
it will be perfectly object-safe.
This does lead to slightly worse ergnomics when using trait objects
(due to that compiler bug with concrete types)
but there really isn't much we can do about that.

So let's start by bringing back our old definition of `LendingIterator`,
but this time under the name `ErasedLendingIterator`:

```rs
pub trait LendingIteratorGats<'a> {
	type Item;
}

pub trait ErasedLendingIterator {
	type Gats: ?Sized + for<'this> LendingIteratorGats<'this>;
	fn erased_next(&mut self) -> Option<<Self::Gats as LendingIteratorGats<'_>>::Item>;
}
```

Next,
we add a blanket implementation of this trait
for all `LendingIterator`s:

```rs
impl<I: ?Sized + LendingIterator> ErasedLendingIterator for I {
	type Gats = dyn for<'this> LendingIteratorGats<
		'this,
		Item = <I as LendingIteratorLifetime<'this>>::Item,
	>;

	fn erased_next(&mut self) -> Option<<Self::Gats as LendingIteratorGats<'_>>::Item> {
		self.next()
	}
}
```

Finally,
we implement the regular `LendingIterator` trait
on all the trait objects we own:

```rs
impl<'this, Gats> LendingIteratorLifetime<'this>
	for dyn '_ + ErasedLendingIterator<Gats = Gats>
where
	Gats: ?Sized + for<'a> LendingIteratorGats<'a>,
{
	type Item = <Gats as LendingIteratorGats<'this>>::Item;
}

impl<Gats> LendingIterator
	for dyn '_ + ErasedLendingIterator<Gats = Gats>
where
	Gats: ?Sized + for<'a> LendingIteratorGats<'a>,
{
	fn next(&mut self) -> Option<<Self as LendingIteratorLifetime<'_>>::Item> {
		self.erased_next()
	}
}

// omitted implementations for all the permutations of auto traits. in a real
// implementation, you'd probably use a macro to generate all 32 versions
// (since there are 5 auto traits)
```

This is fairly standard boilerplate
for defining an object-safe version
of a non-object-safe trait,
so I won't explain it in great detail here.

Great,
let's try it out!
Here,
we can use it to create an iterator over either
windows of size 2
or windows of size 3.

```rs
let mut array = [1, 2, 3, 4];

fn unsize<const N: usize>(array: &mut [i32; N]) -> &mut [i32] {
	array
}

type Gats = dyn for<'a> LendingIteratorGats<'a, Item = &'a mut [i32]>;
type Erased<'iter> = dyn 'iter + ErasedLendingIterator<Gats = Gats>;

let mut iter: Box<Erased<'_>> = if true {
	Box::new(map(windows_mut::<_, 2>(&mut array), unsize))
} else {
	Box::new(map(windows_mut::<_, 3>(&mut array), unsize))
};

while let Some(item) = iter.next() {
    println!("{item:?}");
}
```

and `cargo build` it…

```rs
error: implementation of `LendingIteratorLifetime` is not general enough
   --> src/main.rs:166:3
    |
166 |         Box::new(map(windows_mut::<_, 2>(&mut array), unsize))
    |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ implementation of `LendingIteratorLifetime` is not general enough
    |
    = note: `Map<WindowsMut<'_, i32, 2_usize>, for<'r> fn(&'r mut [i32; 2]) -> &'r mut [i32] {unsize::<2_usize>}>` must implement `LendingIteratorLifetime<'0>`, for any lifetime `'0`...
    = note: ...but it actually implements `LendingIteratorLifetime<'1>`, for some specific lifetime `'1`
```

…ah.
Another cryptic error.

I believe what's happening here
is the same ergnomics issue as faced with [workaround 1]:
There's some compiler bug which makes this not work with concrete types.

So that means all we have to do to fix it
is to move it into a generic function!
And indeed this version does compile:

```rs
fn box_erase<'iter, I>(iter: I) -> Box<Erased<'iter>>
where
	I: 'iter + LendingIterator,
	I: for<'a> LendingIteratorLifetime<'a, Item = &'a mut [i32]>,
{
	Box::new(iter)
}

let mut iter: Box<Erased<'_>> = if true {
	box_erase(map(windows_mut::<_, 2>(&mut array), unsize))
} else {
	box_erase(map(windows_mut::<_, 3>(&mut array), unsize))
};
```

But we can do better than that,
because generics are only _one_ way to erase a value's concrete type:
you can also do it via return-position `[rs] impl Trait`.

```rs
fn funnel_opaque<'iter, I>(iter: I)
	-> impl 'iter + ErasedLendingIterator<Gats = Gats>
where
	I: 'iter + LendingIterator,
	I: for<'a> LendingIteratorLifetime<'a, Item = &'a mut [i32]>,
{
	iter
}

let mut iter: Box<Erased<'_>> = if false {
	Box::new(funnel_opaque(map(windows_mut::<_, 2>(&mut array), unsize)))
} else {
	Box::new(funnel_opaque(map(windows_mut::<_, 3>(&mut array), unsize)))
};
```

And this also works.

If you want to,
you can generalize `funnel_opaque` further
so that it works with any `[rs] &'a mut T` type
instead of just `[rs] &'a mut [i32]`:

```rs
type Gats<T> = dyn for<'a> LendingIteratorGats<'a, Item = &'a mut T>;
type Erased<'iter, T> = dyn 'iter + ErasedLendingIterator<Gats = Gats<T>>;

fn funnel_opaque<'iter, I, T>(iter: I)
	-> impl 'iter + ErasedLendingIterator<Gats = Gats<T>>
where
	T: ?Sized,
	I: 'iter + LendingIterator,
	I: for<'a> LendingIteratorLifetime<'a, Item = &'a mut T>,
{
	iter
}

let mut iter: Box<Erased<'_, [i32]>> = if false {
	Box::new(funnel_opaque(map(windows_mut::<_, 2>(&mut array), unsize)))
} else {
	Box::new(funnel_opaque(map(windows_mut::<_, 3>(&mut array), unsize)))
};
```

But unfortunately you can't generalize it completely to any `LendingIterator`,
because you just run into that compiler bug again.

## Conclusion { #conclusion }

So there we have it -
this technique is,
to my knowledge,
the best way to use lifetime GATs in Rust.
Even once real GATs become stabilized,
I predict it'll likely still be useful
for a long time to come,
so you might want to familiarize yourself with it.

[GATs]: https://github.com/rust-lang/rust/issues/44265
[HRTBs]: https://doc.rust-lang.org/nomicon/hrtb.html
[where hrtb issue]: https://github.com/rust-lang/rust/issues/95268
[this gist]: https://gist.github.com/jix/42d0e4a36ace4c618a59f0ba03be5bf5
[@jix]: https://github.com/jix
[workaround 1]: #dyn-trait-as-a-hkt
