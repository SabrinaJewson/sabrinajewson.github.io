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
	container.append(first, prev, middle, next, last);
	page_controls.push({ first, last, prev, next, middle });
}
const score_header = document.getElementById("score-header");

let current_sort;
let current_filter;
let pages;

function set_sort_and_filter(new_sort, new_filter) {
	current_sort = new_sort;
	current_filter = new_filter;

	let rows;
	if (new_sort !== null) {
		rows = [...original_rows];
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
		rows = [...original_rows];
		score_header.textContent = "Score ↕";
	}
	pages = [];
	for (let i = 0; i < rows.length; i += page_size) {
		pages.push(rows.slice(i, i + page_size));
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

set_sort_and_filter(null, null);

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
	middle.addEventListener("click", () => {
		const text_box = document.createElement("input");
		middle.replaceChildren(text_box);
		let finished = false;
		const finish = () => {
			if (finished) {
				return;
			}
			finished = true;
			let n = parseInt(text_box.value);
			if (isNaN(n)) {
				middle.replaceChildren(current_page);
				return;
			}
			n = Math.max(0, Math.min(pages.length - 1, n));
			if (n !== current_page) {
				set_shown_page(n);
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
