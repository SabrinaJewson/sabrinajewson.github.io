const rows = [...document.getElementById("rows").content.children];
const reviews_table = document.getElementById("reviews");

for (const row of rows) {
	for (const child of row.content.children) {
		reviews_table.append(child.cloneNode(true));
	}
}
