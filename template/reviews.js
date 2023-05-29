"use strict";

const original_rows = [...document.getElementById("rows").content.children];
const page_size = 16;
const reviews_table = document.getElementById("reviews");
const page_controls = [];
for (const container of document.getElementsByClassName("page-controls")) {
	const first = document.createElement("button");
	const last = document.createElement("button");
	const prev = document.createElement("button");
	const next = document.createElement("button");
	const middle = document.createElement("button");
	first.append("0");
	prev.append("<");
	next.append(">");
	first.classList.add("pretty");
	last.classList.add("pretty");
	prev.classList.add("pretty");
	next.classList.add("pretty");
	middle.classList.add("pretty");
	container.append(first, prev, middle, next, last);
	page_controls.push({ first, last, prev, next, middle });
}
const score_header = document.getElementById("score-header");
const [filter_input, filter_clear_button] = document.getElementById("filter").children;

const UNSORTED = "↕";
const ASCENDING = "↑";
const DESCENDING = "↓";
const SORT_BY_SCORE = "Score";
const SORT_BY_DATE = "Date";
const SORTS = [
	{ by: SORT_BY_SCORE, direction: UNSORTED },
	{ by: SORT_BY_SCORE, direction: DESCENDING },
	{ by: SORT_BY_SCORE, direction: ASCENDING },
	{ by: SORT_BY_DATE, direction: DESCENDING },
	{ by: SORT_BY_DATE, direction: ASCENDING },
];

let current_sort;
let current_filter;
let pages;

function set_sort_and_filter(new_sort, new_filter) {
	current_sort = new_sort;
	current_filter = new_filter;
	filter_input.value = new_filter;
	history.replaceState(null, "", location.origin + location.pathname + (
		new_filter === "" ? "" : `?q=${new_filter}`
	));

	let rows;
	if (new_filter !== null) {
		const parts = new_filter.toLowerCase().split(" ");
		rows = original_rows.filter(row => {
			const row_content = row.content.firstElementChild.textContent.toLowerCase();
			return parts.every(part => row_content.includes(part));
		});
	} else {
		rows = [...original_rows];
	}

	const sort_data = SORTS[new_sort];

	let multiplier;
	switch (sort_data.direction) {
		case UNSORTED: multiplier = 0; break;
		case ASCENDING: multiplier = 1; break;
		case DESCENDING: multiplier = -1; break;
	}

	let row_val;
	switch (sort_data.by) {
		case SORT_BY_DATE: {
			row_val = row => {
				const elem = row.content.firstElementChild.getElementsByTagName("time")[0];
				return (elem && multiplier * Date.parse(elem.dateTime)) ?? Infinity;
			};
			break;
		}
		case SORT_BY_SCORE: {
			row_val = row => {
				const elem = row.content.firstElementChild.getElementsByClassName("score")[0];
				return (elem && multiplier * parseFloat(elem.textContent)) ?? Infinity;
			};
			break;
		}
	}

	if (multiplier !== 0) {
		rows.sort((row_a, row_b) => row_val(row_a) - row_val(row_b));
	}
	score_header.textContent = `${sort_data.by} ${sort_data.direction}`;

	pages = [];
	for (let i = 0; i < rows.length; i += page_size) {
		pages.push(rows.slice(i, i + page_size));
	}
	if (pages.length === 0) {
		pages.push([]);
	}
	for (const { last } of page_controls) {
		last.replaceChildren(pages.length - 1);
	}
	set_shown_page(0);
}

let current_page;

function set_shown_page(new_page) {
	reviews_table.replaceChildren();
	for (const row of pages[new_page]) {
		for (const child of row.content.children) {
			reviews_table.append(child.cloneNode(true));
		}
	}

	for (const { first, last, prev, next, middle } of page_controls) {
		if (new_page === 0) {
			first.disabled = true;
			prev.disabled = true;
		} else if (current_page === 0) {
			first.disabled = false;
			prev.disabled = false;
		}
		if (new_page === pages.length - 1) {
			last.disabled = true;
			next.disabled = true;
		} else {
			last.disabled = false;
			next.disabled = false;
		}
		middle.replaceChildren(new_page);
	}

	current_page = new_page;
}

score_header.parentElement.addEventListener("click", () => {
	set_sort_and_filter((current_sort + 1) % SORTS.length, current_filter);
});

for (const { first, last, prev, next, middle } of page_controls) {
	first.addEventListener("click", () => set_shown_page(0));
	last.addEventListener("click", () => set_shown_page(pages.length - 1));
	prev.addEventListener("click", () => set_shown_page(Math.max(0, current_page - 1)));
	next.addEventListener("click", () => set_shown_page(Math.min(pages.length - 1, current_page + 1)));
	let is_text_boxed = false;
	middle.addEventListener("click", () => {
		if (is_text_boxed) {
			return;
		}
		is_text_boxed = true;

		const text_box = document.createElement("input");
		middle.replaceChildren(text_box);
		const finish = () => {
			if (!is_text_boxed) {
				return;
			}
			is_text_boxed = false;

			let n = parseInt(text_box.value);
			if (isNaN(n)) {
				middle.replaceChildren(current_page);
				return;
			}
			n = Math.max(0, Math.min(pages.length - 1, n));
			if (n !== current_page) {
				set_shown_page(n);
			} else {
				middle.replaceChildren(current_page);
			}
		};
		text_box.addEventListener("blur", () => finish());
		text_box.addEventListener("keypress", e => {
			if (e.key === "Enter") {
				finish();
			}
		});
		text_box.focus();
	});
}

filter_input.addEventListener("input", () => set_sort_and_filter(current_sort, filter_input.value));
filter_clear_button.addEventListener("click", () => set_sort_and_filter(current_sort, ""));

set_sort_and_filter(0, (new URLSearchParams(location.search)).get("q") || filter_input.value);
