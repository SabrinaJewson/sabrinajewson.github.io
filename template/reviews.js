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
	if (new_sort !== null) {
		rows.sort((row_a, row_b) => {
			const score_of_row = row => {
				const score_elem = row.content.firstElementChild.getElementsByClassName("score")[0];
				if (score_elem === undefined) {
					return Infinity;
				}
				const score = parseFloat(score_elem.textContent);
				switch (new_sort.direction) {
					case "ascending": return score;
					case "descending": return -score;
					default: throw new Error(`unknown sort direction ${new_sort.direction}`);
				}
			};
			return score_of_row(row_a) - score_of_row(row_b);
		});
		switch (new_sort.direction) {
			case "ascending": { score_header.textContent = "Score ↑"; break; }
			case "descending": { score_header.textContent = "Score ↓"; break; }
		}
	} else {
		score_header.textContent = "Score ↕";
	}
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
	if (current_sort === null) {
		set_sort_and_filter({ direction: "descending" }, current_filter);
	} else if (current_sort.direction === "descending") {
		set_sort_and_filter({ direction: "ascending" }, current_filter);
	} else if (current_sort.direction === "ascending") {
		set_sort_and_filter(null, current_filter);
	}
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

set_sort_and_filter(null, (new URLSearchParams(location.search)).get("q") || filter_input.value);
