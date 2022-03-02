# Test post for testing out things

Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.
Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.
Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur.
Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.

A paragraph with _emphasized text_, **strong text** and ~~strikethrough text~~.
This is a [link](https://rust-lang.org) and below is an image:

![ferris with a trans flag](https://pbs.twimg.com/media/ECtvrfqUYAEjvpB?format=jpg&name=medium)

<small>[Image credit to Karen](https://twitter.com/whoisaldeka/status/1165147725542785025)</small>

<style>img { height: 100px; }</style>

This  
paragraph  
has  
many  
hard  
breaks.

## Lists { #lists }

An unordered list:

- Item 1
- Item 2
- Item 3

An ordered list:

1. Item 1
1. Item 2
1. Item 3
	- Lists can get
		1. Very
			- Nested
		1. In
		1. Markdown

## Block quotes { #blockquotes }

> This is a block quote
>
> That spans multiple lines

## Code blocks { #code-blocks }

### No syntax highlighting { #no-syntax-highlighting }

This is `inline code without syntax highlighting`.

```
fenced code block without syntax highlighting
```

	indented code block without syntax highlighting

### Syntax highlighting { #syntax-highlighting }

This is `[rs] let inline_code = with.syntax(&highlighting)?`.

```rs
fn a() -> Code<Block> {
	with(Syntax {}).highlighting()
}
```

## Tables { #tables }

| A | Table | With |
| - | ----- | ---- |
| The | default | alignment |
| rendered | from | Markdown

| This | table | has | a |
| ---- | :---  | :-: | -: |
| different | alignment | in | each |
| column. | It | also | has |
| | empty | | cells. |
