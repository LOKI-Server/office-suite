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

// Show exact times in the reader's own time zone; the relative text stays as a tooltip.
for (const time of document.querySelectorAll("time[data-ms]")) {
  const date = new Date(Number(time.dataset.ms));
  if (Number.isNaN(date.getTime())) continue;
  time.title = time.textContent;
  time.dateTime = date.toISOString();
  time.textContent = date.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

// Buttons with data-copy="ID" copy that input's value.
document.addEventListener("click", async (event) => {
  const button = event.target.closest("[data-copy]");
  if (!button) return;
  const input = document.getElementById(button.dataset.copy);
  try {
    await navigator.clipboard.writeText(input.value);
  } catch (_) {
    input.select();
    document.execCommand("copy");
  }
  const label = button.textContent;
  button.textContent = "Copied";
  setTimeout(() => { button.textContent = label; }, 1500);
});
