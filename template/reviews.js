const original_rows = [...document.getElementById("rows").content.children];
const page_size = 16;
const total_pages = Math.ceil(original_rows.length / page_size);
const reviews_table = document.getElementById("reviews");
const page_controls = [];
for (const container of document.getElementsByClassName("page-controls")) {
	const prev = document.createElement("div");
	prev.addEventListener("click", () => set_shown_page(Math.max(0, current_page - 1)));
	prev.append("<");
	container.append(prev);

	let buttons = [];
	for (let i = 0; i < total_pages; ++i) {
		const button = document.createElement("div");
		button.append(i);
		button.addEventListener("click", () => set_shown_page(i));
		container.append(button);
		buttons.push(button);
	}

	const next = document.createElement("div");
	next.addEventListener("click", () => set_shown_page(Math.min(total_pages - 1, current_page + 1)));
	next.append(">");
	container.append(next);

	page_controls.push({ prev, next, buttons });
}
const score_header = document.getElementsByClassName("score-header")[0];

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

	for (const { prev, next, buttons } of page_controls) {
		if (new_page === 0) {
			prev.classList.add("disabled");
		} else if (current_page === 0) {
			prev.classList.remove("disabled");
		}
		if (new_page === pages.length - 1) {
			next.classList.add("disabled");
		} else if (current_page === pages.length - 1) {
			next.classList.remove("disabled");
		}
		if (current_page !== undefined) {
			buttons[current_page].classList.remove("selected");
		}
		buttons[new_page].classList.add("selected");
	}

	current_page = new_page;
}

set_sort("native");

score_header.addEventListener("click", () => {
	if (current_sort === "native") {
		set_sort({ direction: "descending" });
	} else if (current_sort.direction === "descending") {
		set_sort({ direction: "ascending" });
	} else if (current_sort.direction === "ascending") {
		set_sort("native");
	}
});
