import { invoke } from "@tauri-apps/api/core";
import "./styles.css";

interface LibraryItem {
  name: string;
  fileCount: number;
}

interface SearchResult {
  collection: string;
  fileName: string;
  relativePath: string;
  startMs: number;
  endMs: number;
  text: string;
  score: number;
  matchKind: "Exact" | "Very close" | "Close" | "Related";
}

const app = document.querySelector<HTMLDivElement>("#app")!;

let library: LibraryItem[] = [];
let currentCollection: LibraryItem | null = null;
let libraryRoot = "";
let searchTimer: number | undefined;
let searchSequence = 0;
let scope: "collection" | "all" = "collection";

const escapeHtml = (value: string) =>
  value.replace(
    /[&<>"']/g,
    (char) =>
      ({
        "&": "&amp;",
        "<": "&lt;",
        ">": "&gt;",
        '"': "&quot;",
        "'": "&#039;",
      })[char] ?? char,
  );

const formatTime = (ms: number) => {
  const hours = Math.floor(ms / 3_600_000);
  const minutes = Math.floor((ms % 3_600_000) / 60_000);
  const seconds = Math.floor((ms % 60_000) / 1000);
  const millis = ms % 1000;

  const hh = hours.toString().padStart(2, "0");
  const mm = minutes.toString().padStart(2, "0");
  const ss = seconds.toString().padStart(2, "0");
  const mmm = millis.toString().padStart(3, "0");

  return `${hh}:${mm}:${ss}.${mmm}`;
};

const collectionIcon = (name: string) => {
  const initials = name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase())
    .join("");
  return initials || "S";
};

function renderLibrary() {
  currentCollection = null;
  document.title = "Subtitle Searcher";

  const cards = library
    .map(
      (item, index) => `
        <button class="library-card" data-index="${index}">
          <span class="library-monogram">${escapeHtml(collectionIcon(item.name))}</span>
          <span class="library-card-copy">
            <strong>${escapeHtml(item.name)}</strong>
            <span>${item.fileCount} ${item.fileCount === 1 ? "subtitle" : "subtitles"}</span>
          </span>
          <span class="chevron" aria-hidden="true">›</span>
        </button>
      `,
    )
    .join("");

  app.innerHTML = `
    <main class="shell">
      <header class="topbar">
        <div>
          <p class="eyebrow">LOCAL LIBRARY</p>
          <h1>Subtitle Searcher</h1>
        </div>
        <button class="icon-button" id="refresh-library" title="Rescan library" aria-label="Rescan library">
          ↻
        </button>
      </header>

      <section class="library-heading">
        <div>
          <h2>Your library</h2>
          <p>Choose a show or movie to search its dialogue.</p>
        </div>
        <span class="library-count">${library.length}</span>
      </section>

      <section class="library-grid">
        ${
          cards ||
          `<div class="empty-state">
            <div class="empty-icon">CC</div>
            <h3>No shows or movies yet</h3>
            <p>Create a folder inside <strong>Shows</strong>, then put .srt or .vtt subtitles in it.</p>
            <code>${escapeHtml(libraryRoot || "Shows/")}</code>
          </div>`
        }
      </section>

      <footer class="library-footer">
        <span>Library folder</span>
        <code title="${escapeHtml(libraryRoot)}">${escapeHtml(libraryRoot)}</code>
      </footer>
    </main>
  `;

  document.querySelector("#refresh-library")?.addEventListener("click", refreshLibrary);

  document.querySelectorAll<HTMLButtonElement>(".library-card").forEach((button) => {
    button.addEventListener("click", () => {
      const item = library[Number(button.dataset.index)];
      if (item) {
        openCollection(item);
      }
    });
  });
}

function openCollection(item: LibraryItem) {
  currentCollection = item;
  scope = "collection";
  document.title = `${item.name} — Subtitle Searcher`;

  app.innerHTML = `
    <main class="shell search-shell">
      <header class="search-header">
        <button class="back-button" id="back-library">‹ <span>Library</span></button>
        <div class="search-title">
          <span class="small-monogram">${escapeHtml(collectionIcon(item.name))}</span>
          <div>
            <h1>${escapeHtml(item.name)}</h1>
            <p>${item.fileCount} ${item.fileCount === 1 ? "subtitle file" : "subtitle files"}</p>
          </div>
        </div>
        <button class="icon-button" id="refresh-current" title="Rescan library" aria-label="Rescan library">↻</button>
      </header>

      <section class="search-panel">
        <label class="search-box">
          <span class="search-symbol">⌕</span>
          <input
            id="search-input"
            type="search"
            autocomplete="off"
            spellcheck="false"
            placeholder="Search a word, expression, or natural sentence…"
          />
          <kbd>Enter</kbd>
        </label>

        <div class="scope-row">
          <span>Search in</span>
          <div class="segmented" role="group" aria-label="Search scope">
            <button class="segment active" data-scope="collection">This title</button>
            <button class="segment" data-scope="all">Entire library</button>
          </div>
        </div>
      </section>

      <section id="results-area" class="results-area">
        <div class="search-placeholder">
          <div class="placeholder-mark">“ ”</div>
          <h2>Search the dialogue</h2>
          <p>Exact matches come first, but the search also understands common inflections, reordered expressions, synonyms, and fuzzy wording.</p>
        </div>
      </section>
    </main>
  `;

  document.querySelector("#back-library")?.addEventListener("click", renderLibrary);
  document.querySelector("#refresh-current")?.addEventListener("click", async () => {
    await refreshLibrary();
    const fresh = library.find((entry) => entry.name === item.name);
    if (fresh) openCollection(fresh);
  });

  const input = document.querySelector<HTMLInputElement>("#search-input")!;
  input.focus();
  input.addEventListener("input", () => scheduleSearch(input.value));
  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      window.clearTimeout(searchTimer);
      void performSearch(input.value);
    }
  });

  document.querySelectorAll<HTMLButtonElement>(".segment").forEach((button) => {
    button.addEventListener("click", () => {
      scope = button.dataset.scope === "all" ? "all" : "collection";
      document.querySelectorAll(".segment").forEach((segment) => segment.classList.remove("active"));
      button.classList.add("active");
      void performSearch(input.value);
    });
  });
}

function scheduleSearch(query: string) {
  window.clearTimeout(searchTimer);
  if (!query.trim()) {
    renderSearchPlaceholder();
    return;
  }

  searchTimer = window.setTimeout(() => {
    void performSearch(query);
  }, 180);
}

function renderSearchPlaceholder() {
  const area = document.querySelector<HTMLDivElement>("#results-area");
  if (!area) return;

  area.innerHTML = `
    <div class="search-placeholder">
      <div class="placeholder-mark">“ ”</div>
      <h2>Search the dialogue</h2>
      <p>Try a word like <strong>counter</strong> or a phrase like <strong>take the trash out</strong>.</p>
    </div>
  `;
}

async function performSearch(query: string) {
  const trimmed = query.trim();
  const area = document.querySelector<HTMLDivElement>("#results-area");
  if (!area || !currentCollection) return;

  if (!trimmed) {
    renderSearchPlaceholder();
    return;
  }

  const sequence = ++searchSequence;
  area.innerHTML = `
    <div class="searching">
      <span class="spinner"></span>
      <span>Searching subtitles…</span>
    </div>
  `;

  try {
    const results = await invoke<SearchResult[]>("search_subtitles", {
      collection: scope === "collection" ? currentCollection.name : null,
      query: trimmed,
      limit: 120,
    });

    if (sequence !== searchSequence) return;
    renderResults(results, trimmed);
  } catch (error) {
    if (sequence !== searchSequence) return;
    area.innerHTML = `
      <div class="error-state">
        <h3>Search failed</h3>
        <p>${escapeHtml(String(error))}</p>
      </div>
    `;
  }
}

function renderResults(results: SearchResult[], query: string) {
  const area = document.querySelector<HTMLDivElement>("#results-area");
  if (!area) return;

  if (results.length === 0) {
    area.innerHTML = `
      <div class="empty-results">
        <h2>No close results</h2>
        <p>Nothing in the selected subtitles was similar enough to <strong>“${escapeHtml(query)}”</strong>.</p>
      </div>
    `;
    return;
  }

  const cards = results
    .map(
      (result) => `
        <article class="result-card">
          <div class="result-topline">
            <div class="result-file">
              ${
                scope === "all"
                  ? `<span class="collection-label">${escapeHtml(result.collection)}</span>`
                  : ""
              }
              <strong title="${escapeHtml(result.relativePath)}">${escapeHtml(result.fileName)}</strong>
            </div>
            <span class="match-badge match-${result.matchKind.toLowerCase().replaceAll(" ", "-")}">
              ${escapeHtml(result.matchKind)}
            </span>
          </div>

          <div class="timestamp">
            ${formatTime(result.startMs)}
            <span>→</span>
            ${formatTime(result.endMs)}
          </div>

          <blockquote>${escapeHtml(result.text)}</blockquote>
        </article>
      `,
    )
    .join("");

  area.innerHTML = `
    <div class="results-heading">
      <span><strong>${results.length}</strong> ${results.length === 1 ? "result" : "results"}</span>
      <span>Closest matches first</span>
    </div>
    <div class="results-list">${cards}</div>
  `;
}

async function refreshLibrary() {
  try {
    [libraryRoot, library] = await Promise.all([
      invoke<string>("library_root"),
      invoke<LibraryItem[]>("scan_library"),
    ]);

    if (currentCollection) {
      const fresh = library.find((item) => item.name === currentCollection?.name);
      if (!fresh) {
        renderLibrary();
      }
    } else {
      renderLibrary();
    }
  } catch (error) {
    app.innerHTML = `
      <main class="shell">
        <div class="fatal-error">
          <h1>Subtitle Searcher</h1>
          <h2>Could not open the library</h2>
          <p>${escapeHtml(String(error))}</p>
        </div>
      </main>
    `;
  }
}

void refreshLibrary();
