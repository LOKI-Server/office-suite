// Autosave shared by the suite's editors.
//
// OfficeAutosave({url, csrf, version, noun, body, focus}) saves
// {title, version, body: body()} to `url` about a second after the last change
// (at most five seconds after the first), on Ctrl+S, and when the page is
// hidden. A 409 answer shows the conflict banner. The page provides the
// #doc-title input, #save-status and the #conflict banner.
"use strict";
window.OfficeAutosave = function (options) {
  const titleInput = document.getElementById("doc-title");
  const statusEl = document.getElementById("save-status");
  const conflictBar = document.getElementById("conflict");
  const conflictText = document.getElementById("conflict-text");
  const conflictReload = document.getElementById("conflict-reload");
  const conflictOverwrite = document.getElementById("conflict-overwrite");

  let version = options.version;
  let dirty = false;
  let saving = false;
  let blocked = false; // waiting for the person after a conflict or a rejected save
  let saveTimer = null;
  let firstChangeAt = 0;
  let retryDelay = 2000;
  let leaving = false;

  function setStatus(text, error) {
    statusEl.textContent = text;
    statusEl.classList.toggle("error", !!error);
  }

  function payload() {
    return JSON.stringify({ title: titleInput.value, version, body: options.body() });
  }

  function post(body, keepalive) {
    return fetch(options.url, {
      method: "POST",
      keepalive,
      credentials: "same-origin",
      headers: { "content-type": "application/json", "x-csrf-token": options.csrf },
      body,
    });
  }

  function scheduleSave() {
    clearTimeout(saveTimer);
    const waited = firstChangeAt ? Date.now() - firstChangeAt : 0;
    saveTimer = setTimeout(save, waited > 5000 ? 0 : 1000);
  }

  async function save() {
    clearTimeout(saveTimer);
    if (saving || !dirty || blocked) return;
    saving = true;
    dirty = false;
    firstChangeAt = 0;
    setStatus("Saving…");
    let retry = false;
    try {
      const response = await post(payload(), false);
      const isJson = (response.headers.get("content-type") || "").includes("application/json");
      if (response.ok && isJson) {
        const saved = await response.json();
        version = saved.version;
        retryDelay = 2000;
        setStatus(dirty ? "Unsaved changes" : "Saved");
      } else if (response.status === 409 && isJson) {
        dirty = true;
        showConflict(await response.json());
      } else if (response.ok || response.status === 401) {
        // A redirect to sign-in answers with an HTML page.
        dirty = true;
        blocked = true;
        setStatus("Signed out. Sign in again in another tab, then press Ctrl+S.", true);
      } else {
        dirty = true;
        blocked = response.status < 500;
        retry = !blocked;
        const message = (await response.text()).trim();
        setStatus(message.length < 200 && message ? message : "Couldn't save.", true);
      }
    } catch (_) {
      dirty = true;
      retry = true;
      setStatus("Offline. Retrying…", true);
    } finally {
      saving = false;
    }
    if (retry) {
      saveTimer = setTimeout(save, retryDelay);
      retryDelay = Math.min(retryDelay * 2, 30000);
    } else if (dirty && !blocked) {
      scheduleSave();
    }
  }

  function saveNow() {
    blocked = blocked && !conflictBar.hidden;
    dirty = dirty || !saving;
    save();
  }

  function showConflict(conflict) {
    blocked = true;
    conflictText.textContent = conflict.trashed
      ? `This ${options.noun} was moved to the trash, so your latest changes weren't saved.`
      : `This ${options.noun} was changed somewhere else, so your latest changes weren't saved.`;
    conflictOverwrite.hidden = !!conflict.trashed;
    conflictOverwrite.onclick = () => {
      version = conflict.version;
      blocked = false;
      conflictBar.hidden = true;
      dirty = true;
      save();
    };
    conflictBar.hidden = false;
    setStatus("Not saved", true);
  }

  // Call after every change to the content or title.
  function changed() {
    if (!firstChangeAt) firstChangeAt = Date.now();
    dirty = true;
    if (!blocked) {
      setStatus("Unsaved changes");
      scheduleSave();
    } else if (conflictBar.hidden) {
      // A rejected save may succeed once the content changes.
      blocked = false;
      scheduleSave();
    }
  }

  conflictReload.addEventListener("click", () => {
    leaving = true;
    window.location.reload();
  });

  window.addEventListener("beforeunload", (event) => {
    if (leaving || (!dirty && !saving)) return;
    const body = payload();
    if (!blocked && body.length < 60000) post(body, true).catch(() => {});
    event.preventDefault();
    event.returnValue = "";
  });
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") save();
  });
  document.addEventListener("keydown", (event) => {
    if ((event.ctrlKey || event.metaKey) && !event.altKey && event.key.toLowerCase() === "s") {
      event.preventDefault();
      saveNow();
    }
  });

  titleInput.addEventListener("input", () => {
    document.title = (titleInput.value.trim() || "Untitled " + options.noun) + " · Office Suite";
    changed();
  });
  titleInput.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      options.focus();
    }
  });

  return { changed };
};
