{
	"published": "2024-09-22"
}

# “Truly Hygienic” Let Statements in Rust

Remon is a responsible library developer.
She cares about stability, flexibility and correctness,
using whichever tools are presently accessible to achieve those goals.
Her authored libraries feature automated testing and extensive documentation;
she allots design decisions rationale;
she knows her language features and traditions
and how to apply them to best effect.

And, somewhere to be discovered bound in the tangle of `.rs` files,
there is Remon herself,
tranquil and yet focused,
meticulously crafting, polishing, studying and crafting again,
a component she forsees to ease the life of her users,
provides ergonomics inaccessible by traditional methods,
brings to life the great gift of syntax
without glue added to the cogs of the build process –
a declarative macro.

Refined and learned code-witch she is,
Remon is keenly aware of Rust Cultures and Traditions,
and so in keeping,
would do nothing but summon a monstrous
(documented, without doubt, but monstrous nonetheless)
tornado of dollar signs and brackets,
one whose gales would surely lift up and send flying
a meek blog post as this one.
Have sympathy!
I cannot handle that –
I must admit I have not even implemented `[rs] Send`,
so the results could verge on disastrous.
But a trained magician knows better than to create a beast they cannot tame,
and so for this chronicle
it is simplified to a wisp of its wild self –
one where you must excuse the apparent folly of its existence –
as follows:

```rs
macro_rules! oh_my {
	() => {
		let Ok(x) = read_input() else { return Err(Error) };
		$crate::process(x);
	};
}
```

Remon is a responsible library developer,
and understands that all humans will make mistakes –
and so she has solicited the services of a good friend, Wolfie,
to comment on this slice of code.

Well, Wolfie says, this macro is very impressive feat,
and shall surely ease the lives of our users,
provide ergonomics inaccessible by traditional methods,
and bring to life the great gift of syntax
without glue added to the cogs of the build process.
But I do have one concern –
the `[rs] let` in this macro is not hygienic.

Now, Remon has read her literature,
and knows that Rust macros _are_ hygienic with regards to locals –
they are guaranteed not to interfere with variables of the caller’s scope
unless the variable’s name is explicitly passed in.

Is that so?, asks Remon.
You and I both know that Rust macros use mixed-site hygiene.
But I trust your experience as a developer and respect you as a person,
so I will approach this incongruence with curiosity rather than dismissal.
Thus I must ask you:
Whatever do you mean?

Wolfie thinks for a second,
and concludes this point best communicated through the medium of code.
So he quickly types out a demo
of a certain way of use causing bugs:

```rs
const x: &str = "26ad109e6f4d14e5cc2c2ccb1f5fb497abcaa223";
oh_my!();
```

And upon entering input that is not
the latest commit hash of the greatest Rust library of all time,
Remon is dismayed and ashamed to discover that the code,
incorrectly, results in an error.
But it’s at least not hard to discover _why_:
in the line containing `[rs] let Ok(x) =`,
`x` is a identifier pattern,
which means it can either refer to a constant if the constant is in scope,
or create a new variable otherwise.
Of course, the macro _expects_ the latter to happen,
but since constants are items, and thus unlike variables are unhygienic,
if there is a constant `x` at the call site,
it will be used instead.
So our pattern becomes equivalent to `[rs] Ok("26ad109…")`,
which will of course reject any value that is not
the latest commit hash of the greatest Rust library of all time,
resulting in silent bugs.

Okay, thinks Remon.
I know of a way to fix this:
the pattern `IDENT @ PATTERN`
will unambiguously have `IDENT` bound as a variable,
never to be treated as a constant.
Since there are no other restrictions to be placed on the data,
our `PATTERN` can simply be a wildcard – `_`.
So that’s what she does:

```rs
macro_rules! oh_my {
	() => {
		let Ok(x @ _) = read_input() else { return Err(Error) };
		$crate::process(x);
	};
}
```

But Wolfie is still not pleased,
and Remon is still surprised,
because now there is a compilation error.

```
error[E0530]: let bindings cannot shadow constants
 --> src/main.rs:3:10
  |
3 |         let Ok(x @ _) = read_input() else { return Err(Error) };
  |                ^ cannot be named the same as a constant
...
8 |     const x: &str = "TODO";
  |     ---------------------- the constant `x` is defined here
9 |     oh_my!();
  |     -------- in this macro invocation
  |
```

This is of course not as bad as buggy behaviour,
but Wolfie knows that Remon is a responsible library developer
who cares about flexibility and correctness,
and it is unpredicable that the macro would suddenly start failing
just because of some constants that happen to be there at the call site.

Remon has never seen this error before,
but remains undeterred.
After all, there is one more trick up her sleeve:
although `[rs] let` bindings cannot shadow constants,
those two do not account for every member of the value namespace.
Functions are a member just as well.
And functions, unlike `[rs] const`s, have the property
that they _can_ be shadowed –
and by virtue of being an item,
they may shadow the latter as well
(if introduced in a smaller scope).

So, she introduces that new scope into her macro,
and inside it, defines a dummy function.
As it happens, functions are never valid in patterns,
and so the `x @ _` trick is no longer needed.

```rs
macro_rules! oh_my {
	() => {{
        #[allow(dead_code)]
        fn x() {}
		let Ok(x) = read_input() else { return Err(Error) };
		$crate::process(x);
    }};
}
```

And despite Wolfie’s attempts to break it,
this iteration remains hygienic
even in the presence of strange environments.

But Remon isn’t satisfied.
Because now, being the responsible library developer she is,
whenever she uses this trick, she must document it.
And she has to introduce a shadowing helper function
for every single identifier used in the macro –
something that is very easy to forget,
negating the benefit of using this trick in the first place.
It increases her codebase’s size,
in an already-complex macro,
for a gain that seems marginal at best.

And so,
against her instincts to be fully correct,
Remon turns to Wolfie and says, plainly, _No_.
With the incantation of a `[sh] git reset`,
she erases these changes from history,
choosing instead to live in the ignorant bliss
of very-slightly-unhygienic declarative macros.

After all, who names constants in lowercase anyway?
