{
	"published": "2023-07-18",
	"updated": "2023-07-22"
}

# Why the “Null” Lifetime Does Not Exist

This post originated from an interesting conversation had [on the Rust community Discord][discord]
the other day, in which a user asks:

> Does `[rs] 'static` have an opposite? Zero lifetime that’s shorter than anything?

Details of the question are not relevant,
but intuitively the question does make sense.
After all, Rust already has `[rs] 'static`,
representing a lifetime that is longer than or equal to all other lifetimes —
so why wouldn’t there be a counterpart,
maybe called `[rs] 'empty` or `[rs] 'null`,
that is shorter than every other lifetime?
To the type theorists out there, if `[rs] 'static` is our bottom type
(as it can become any lifetime),
wouldn’t there also hypothetically be some top type
that any lifetime can become?

The short answer is “not in any way that allows you to construct a value with the lifetime”.
Rust’s type system, as this post will show,
is designed with the assumption that given a valid lifetime,
you can always make a shorter one,
so any hypothetical `[rs] 'null` lifetime would
have to be non-constructible in the first place.

But why is that?
Well, that takes a bit of explaining,
but first I’d like to take a bit of a detour
into the world of self-referential types…

## An Overlong Interlude Where I Am Increasingly Pedantic About Self-Referential Types { #self-referential-types }

(This is going somewhere, I promise.)

You may often hear it be said that
Rust does not support self-referential types.
This is typically in response to a beginner attempting code like the following
and thoroughly confusing themselves:

```rs
struct OhNo {
	base_string: String,
	parts: Vec<&'??? str>,
}
```

Where `parts` is supposed to borrow from `base_string`.
The beginner of course expects this to be possible
but has no clue what lifetime to write in there.

Inevitably, you, the seasoned Rustacean,
will put on a grave expression
and slowly shake your head in frank resignation.
Then, bearing the bad news
like a parent informing their child of the truth about Santa Claus,
you say:

> Rust does not support self-referential types.

And the beginner’s dreams are crushed,
for no matter how much they may argue against it,
they will eventually have to accept the tragic truth
such an abstraction is simply not possible.

But we’re programmers here,
and we like things to be precise.
And if this interaction should occur in any technical space,
it is quite expected that some other user be rather pleased with themselves by chiming in:
But what about `async`?

Well, they’re not _wrong_.
So what about it?

If you’ve ever worked with `async` before,
you will know that futures declared with `async` blocks
have the ability to borrow values across `.await` points.
For example:

```rs
let future = async {
	let data = 5;
	let r = &data;
	something_else().await; // Point A
	println!("{r}");
};
```

The above future, once it suspends for the first time at point A,
has to store all of the data required for resumation
in its type.
This means that we have to store both `data` and `r` in the future —
`r` needs to be kept because we directly access it,
and `data` needs to be kept since otherwise `r`’s reference would dangle into thin air.
But `r` also needs to reference `data`,
which means
one part of the type needs to reference another —
meeting exactly the definition of a self-referential type!

Okay then, you say, let’s refine the statement.
`async` blocks can indeed be self-referential,
but that’s not particularly useful
because we can’t extract any data from them
beyond the very limited `Future` interface.
So we restrict our claim:

> Rust does not support self-referential _data structures_.

This is better but still not quite true,
because you can simply make a `[rs] static` item that depends on itself:

```rs
struct SelfRef {
	this: &'static Self,
}
static SELF_REF: SelfRef = SelfRef { this: &SELF_REF };
```

Well that’s just being pedantic now.
But fine, let’s adjust the claim:

> Rust does not support _non-static_ self-referential data structures.

Except, with a little interior mutability trickery with `Cell` and `Option`
to get around the acyclic nature of runtime execution,
this same trick _also_ works on the stack:

```rs
struct SelfRef<'this> {
	this: Cell<Option<&'this Self>>,
}

fn main() {
	let self_ref = SelfRef {
		this: Cell::new(None),
	};
	self_ref.this.set(Some(&self_ref));
}

use std::cell::Cell;
```

[Try it yourself][play] — although it might be surprising, this compiles just fine,
and does indeed result in a `[rs] struct` that technically references itself.

> Edit (2023-07-22):
> After this blog post was pubished, [Daniel Henry-Mantilla] helpfully pointed out that
> you don’t even need interior mutability to make a stack self-referential struct like this,
> so long as you’re willing to sacrifice having a _literal_ self-reference (`[rs]&Self`)
> for a reference to an earlier field.
> Specifically, the following code just works:
> ```rs
> struct SelfRef<'this> {
> 	a: i32,
> 	b: &'this i32,
> }
> fn main() {
> 	let mut self_ref = SelfRef { a: 37, b: &0 };
> 	self_ref.b = &self_ref.a;
> }
> ```
>
> The resulting `[rs] struct` exhibits the same behaviour we talk about later in this section,
> but it’s worth putting in this example as a more “pure” demonstration of the same effect.
> Thanks, Yandros!

So, did we do it?
Have we solved the years-long problem of self-referential types?

Well, of course not,
because this approach comes with one huge problem
that is a deal-breaker for almost all real-life situations:
the resulting value cannot be moved or uniquely borrowed for the rest of its lifetime.
Even if we do something as trivial and innocuous as dropping it,
we start to see the issue.

```diff
struct SelfRef<'this> {
	this: Cell<Option<&'this Self>>,
}

fn main() {
	let self_ref = SelfRef {
		this: Cell::new(None),
	};
	self_ref.this.set(Some(&self_ref));
+	drop(self_ref);
}

use std::cell::Cell;
```

```
error[E0505]: cannot move out of `self_ref` because it is borrowed
  --> src/main.rs:10:10
   |
6  |     let self_ref = SelfRef {
   |         -------- binding `self_ref` declared here
...
9  |     self_ref.this.set(Some(&self_ref));
   |                            --------- borrow of `self_ref` occurs here
10 |     drop(self_ref);
   |          ^^^^^^^^
   |          |
   |          move out of `self_ref` occurs here
   |          borrow later used here

For more information about this error, try `rustc --explain E0505`.
```

And with unique borrowing we get a similar error:

```diff
-	let self_ref = SelfRef {
+	let mut self_ref = SelfRef {
		this: Cell::new(None),
	};
-	drop(self_ref);
+	&mut self_ref.this;
```

```
error[E0502]: cannot borrow `self_ref.this` as mutable because it is also borrowed as immutable
  --> src/main.rs:10:5
   |
9  |     self_ref.this.set(Some(&self_ref));
   |                            --------- immutable borrow occurs here
10 |     &mut self_ref.this;
   |     ^^^^^^^^^^^^^^^^^^
   |     |
   |     mutable borrow occurs here
   |     immutable borrow later used here

For more information about this error, try `rustc --explain E0502`.
```

Of course, this makes perfect sense.
If we _were_ allowed to uniquely borrow the `self_ref` value
we could trivially use that to produce a `[rs] &mut` and `&` to the same location,
which is a textbook case of UB!

```rs
let mut self_ref = SelfRef { this: Cell::new(None) };
self_ref.this.set(Some(&self_ref));
let reference_1: &SelfRef<'_> = self_ref.this.get().unwrap();
let reference_2: &mut SelfRef<'_> = &mut self_ref;
// Oops, UB!
```

These examples are quite abstract,
but they show that you are barred from doing
basically anything useful with the value,
including returning it from functions
or setting any of its fields without interior mutability.
Well what did I expect, we just can’t have nice things.

At least we can improve our claim:

> Rust does not support _movable_ self-referential data structures.

Surely we’re done now?
Well, for the purposes of the main point of the post we are,
but since I’ve started this game
I feel only obliged to indulge in this pedantry to its natural terminus.
So yes, let’s continue…

Our next counterexample is that
C supports movable self-referential data structures.
So if Rust can’t do this, does this mean Rust is inherently less powerful than C?
Well no, of course not, we were just only considering _safe_ code up until now.
You can do anything that C can with a little `[rs] unsafe`,
so let’s add that qualifier:

> Rust does not support _safe_ movable self-referential data structures.

But then we can’t ignore one of Rust’s most powerful features,
the wrapping of `[rs] unsafe` code with safe code.
That is to say,
one can create _safe abstractions_ over what the C code would do
to enable this kind of thing with safe code,
through dependencies that use `[rs] unsafe`.

As it turns out, this kind of thing is easier said than done.
The original attempts at these,
`owning_ref` and `rental`, are now both unsound and unmaintained;
[`yoke`] is also unsound in two separate ways
([1], [2]; although neither are as of today considered exploitable)
and only [`ouroboros`] has managed to fix all the issues.
But it is at least _possible_,
so we can arrive at our final (really final this time, I promise) _true_ statement:

> Rust does not natively support safe movable self-referential data structures.

Well isn’t that a mouthful?

## Always A Shorter Lifetime { #always-a-shorter-lifetime }

Let’s go back to the code example from before,
where Rust prevented us from causing UB with our stack-based self-referential type.

```rs
let mut self_ref = SelfRef { this: Cell::new(None) };
self_ref.this.set(Some(&self_ref));
let reference_1: &SelfRef<'_> = self_ref.this.get().unwrap();
let reference_2: &mut SelfRef<'_> = &mut self_ref;
//                                  ^^^^^^^^^^^^^ Compiler error!
```

This is _weird_, isn’t it?
Because suppose we delete the second line —
then it all compiles just fine,
that’s just basic Rust borrowing rules.
So what is up with that line?
How can one usage of a value, involving only that value,
get it into this weird twilight state
where you can normally borrow
but not uniquely borrow or move no matter what you do?

We know that calling any normal function on this value
would _not_ put it in that twilight state:

```rs
fn uwu(_: &SelfRef<'_>) {}

let mut self_ref = SelfRef { this: Cell::new(None) };
uwu(&self_ref);
let reference_1: &SelfRef<'_> = self_ref.this.get().unwrap();
let reference_2: &mut SelfRef<'_> = &mut self_ref; // Compiles just fine!
```

So this might lead you to believe that
this is some special case in the Rust compiler,
that it detected we were building a self-referential type
and intervened personally to protect us.
But one of the beauties of the borrow checker is that’s it’s _not_,
and we can show that if we
first desugar the lifetimes of `uwu`,
and then try to actually construct the self-referential type within it:

```rs
fn uwu<'a, 'b>(self_ref: &'a SelfRef<'b>) {
	self_ref.this.set(Some(&self_ref));
}
```

```
error: lifetime may not live long enough
 --> src/main.rs:4:2
  |
3 | fn uwu<'a, 'b>(self_ref: &'a SelfRef<'b>) {
  |        --  -- lifetime `'b` defined here
  |        |
  |        lifetime `'a` defined here
4 |     self_ref.this.set(Some(&self_ref));
  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ argument requires that `'a` must outlive `'b`
  |
  = help: consider adding the following bound: `'a: 'b`
```

The fact than an error occurred at all first tells us that
there is some material difference between
using our magic line we had and calling the `uwu` function
as we’ve currrently defined it.
The clue to this difference can be found in the compiler help message:

> consider adding the following bound: `'a: 'b`

So we can do that, and recompile:

```rs
fn uwu<'a: 'b, 'b>(self_ref: &'a SelfRef<'b>) {
	self_ref.this.set(Some(&self_ref));
}

let mut self_ref = SelfRef { this: Cell::new(None) };
uwu(&self_ref);
let reference_1: &SelfRef<'_> = self_ref.this.get().unwrap();
let reference_2: &mut SelfRef<'_> = &mut self_ref;
//                                  ^^^^^^^^^^^^^ error: cannot borrow `self_ref` as mutable
//                                                because it is also borrowed as immutable
```

The same error as before!
This means we’ve perfectly been able to extract
the underlying “borrowing behaviour” behind the line
`[rs] self_ref.this.set(Some(&self_ref))`
into a function,
which gives us clues as to what’s really going on here.
Since we now have the right signature,
we can even delete the body of `uwu` and observe that the error remains the same:

```rs
fn uwu<'a: 'b, 'b>(_: &'a SelfRef<'b>) {}
```

Recall that `:` in lifetimes means “outlives” or “lives at least as long as”.
Therefore,
the generic parameter section `[rs] <'a: 'b, 'b>` of `uwu` tells us
that it operates on the lifetimes
- `[rs] 'a`, which is the same length or longer than, `[rs] 'b`;
- and `[rs] 'b`, which can be any lifetime.

You can also use pure logic to reach the conclusion that
a function accepting `[rs] &'a SelfRef<'b>` where `[rs] 'a: 'b`
is enough to construct a self-referential type,
and thus is also enough to prevent any future moves or unique borrows:
the reference type held inside the `[rs] SelfRef<'b>` is a `[rs] &'b Self`,
but `[rs] Self` in this context is `[rs] SelfRef<'b>`,
so therefore procuring a `[rs] &'b SelfRef<'b>` is sufficient to fill that field in.
If we then have some `[rs] &'a SelfRef<'b>` where `[rs] 'a: 'b`,
as it’s always valid to treat objects as living shorter then they actually do,
it can be implicitly converted into the `[rs] &'b SelfRef<'b>` as desired.

So what was all this about?
Well really,
it was just a long and roundabout way to demonstrate to you
a theorem, in the mathematical sense, that holds in Rust:

> When you have some type with an invariant lifetime parameter `[rs] T<'b>`
> and you borrow it with the lifetime `[rs] 'a` such that `[rs] 'a` outlives `[rs] 'b` (producing `[rs] &'a T<'b>`),
> one is prevented from moving the value thereafter.

(you might notice the presence of the qualifier “invariant” there;
this is another thing I won’t go into
because it’s not that relevant right now,
but it is necessary for the theorem to hold).

We can then take the [contrapositive] of this theorem,
giving us the corollary:

> If one is able to move some type with an invariant lifetime parameter `[rs] T<'b>` after borrowing it,
> then the lifetime which it was borrowed for is strictly shorter than `[rs] 'b`
> (as if it outlived `[rs] 'b`, one would not have been able to move it).

You might be able to see where this is going now.
Take the below code, which compiles:

```rs
// `Cell` is used to make T invariant in `'b`
type T<'b> = Cell<&'b ()>;
fn owo<'b>(mut value: T<'b>) {
	let reference = &value;
	drop(value);
}
use std::cell::Cell;
```

Here we have a function `owo`, accepting some `T<'b>`, borrowing it, and then moving it.
This satisfies all the conditions to apply the theorem above,
which tells us that the duration `reference` borrowed `value` for
**must** be a lifetime that is strictly shorter than `[rs] 'b`.

But as `[rs] 'b` was a lifetime parameter to the function `owo`,
we know that it could have been _any_ lifetime —
it’s not constrained in any way.
This gives the final result for this section:

> Given any lifetime parameter `[rs] 'b`,
> it must be possible to construct a reference whose lifetime
> is required to live strictly shorter than `[rs] 'b`
> in order for Rust to be sound.

Or, in other words,

> There is always a shorter lifetime.

And this is the reason why the `[rs] 'null` lifetime doesn’t exist,
at least in its naïve form.
Because if it did exist,
and if you could pass it to functions,
those functions could always use the trick outlined above
to construct a lifetime that must be shorter.
This leaves us with only two possibilities:

1. `[rs] 'null` is not actually shorter than every other lifetime, defeating its purpose;
1. Rust is unsound.

Of course, this doesn’t not rule out a hypothetical “opposite of `[rs] 'static`” existing entirely;
merely, it proves that it must not be allowed to actually construct a variable with this lifetime.
[dtolnay’s 2017 proposal for the `[rs] 'void` lifetime][2017]
(which to my knowledge was unfortunately never pursued after that initial thread)
is an example of the way in which `[rs] 'static` _could_ have an opposite:
it can be useful in traits as he shows,
but it can never actually be constructed
because it’s so short that any value containing a `[rs] &'void`
would live longer than `[rs] 'void`,
and thus would be disallowed.

This is quite counterintuitive,
as after all if one can never construct a `[rs] 'static` reference to a stack value,
surely one would always be able to construct a `[rs] 'void` reference to a stack value —
but as you’ve seen,
it’s the only way for Rust’s borrow checker to still be sound.

[discord]: https://discord.com/channels/273534239310479360/818964227783262209/1130515943756406874
[play]: https://play.rust-lang.org/?version=stable&mode=debug&edition=2021&gist=6fb49accec79ec9903e7753c8912159d
[1]: https://github.com/unicode-org/icu4x/issues/2095
[2]: https://github.com/unicode-org/icu4x/issues/3696
[`yoke`]: https://docs.rs/yoke
[`ouroboros`]: https://docs.rs/ouroboros
[contrapositive]: https://en.wikipedia.org/wiki/Contraposition
[2017]: https://internals.rust-lang.org/t/opposite-of-static/5128
[Daniel Henry-Mantilla]: https://github.com/danielhenrymantilla
