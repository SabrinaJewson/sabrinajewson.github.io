const original_rows = [...document.getElementById("rows").content.children];
const page_size = 16;
const total_pages = Math.ceil(original_rows.length / page_size);
const reviews_table = document.getElementById("reviews");
const page_controls = [];
for (const container of document.getElementsByClassName("page-controls")) {
	const first = document.createElement("button");
	first.append("0");
	first.addEventListener("click", () => set_shown_page(0));

	const last = document.createElement("button");
	last.append(total_pages - 1);
	last.addEventListener("click", () => set_shown_page(total_pages - 1));

	const prev = document.createElement("button");
	prev.append("<");
	prev.addEventListener("click", () => set_shown_page(Math.max(0, current_page - 1)));

	const next = document.createElement("button");
	next.append(">");
	next.addEventListener("click", () => set_shown_page(Math.min(total_pages - 1, current_page + 1)));

	const middle = document.createElement("button");
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
			n = Math.max(0, Math.min(total_pages - 1, n));
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

	container.append(first, prev, middle, next, last);
	page_controls.push({ first, last, prev, next, middle });
}
const score_header = document.getElementById("score-header");

let current_sort;
let pages;

function set_sort(new_sort) {
	let rows = [...original_rows];
	if (new_sort !== "native") {
		rows.sort((row_a, row_b) => {
			const score_of_row = row => {
				const score_elem = row.content.firstElementChild.getElementsByClassName("score")[0];
				if (score_elem === undefined) {
					return -1;
				}
				return parseFloat(score_elem.textContent);
			};
			const a = score_of_row(row_a);
			const b = score_of_row(row_b);
			switch (new_sort.direction) {
				case "ascending": return a - b;
				case "descending": return b - a;
				default: throw new Error(`unknown sort direction ${new_sort.direction}`);
			}
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
	set_shown_page(0);
	current_sort = new_sort;
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
		} else if (current_page === pages.length - 1) {
			last.disabled = false;
			next.disabled = false;
		}
		middle.replaceChildren(new_page);
	}

	current_page = new_page;
}

set_sort("native");

score_header.parentElement.addEventListener("click", () => {
	if (current_sort === "native") {
		set_sort({ direction: "descending" });
	} else if (current_sort.direction === "descending") {
		set_sort({ direction: "ascending" });
	} else if (current_sort.direction === "ascending") {
		set_sort("native");
	}
});
