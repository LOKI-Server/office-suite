// Small behaviours for list pages; the page works without them.
"use strict";

// Ask before destructive form posts marked with data-confirm.
document.addEventListener("submit", (event) => {
  const message = event.target.dataset && event.target.dataset.confirm;
  if (message && !window.confirm(message)) event.preventDefault();
});

document.addEventListener("click", (event) => {
  if (event.target.closest("[data-print]")) window.print();
});

// Close an open overflow menu when clicking elsewhere or pressing Escape.
function closeMenus(except) {
  for (const menu of document.querySelectorAll("details.menu[open]")) {
    if (menu !== except) menu.open = false;
  }
}
document.addEventListener("click", (event) => closeMenus(event.target.closest("details.menu")));
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") closeMenus(null);
});
