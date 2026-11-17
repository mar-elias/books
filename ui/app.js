const viewState = { kind: "all" };
let books = [];
let authors = [];
let categories = [];
let tags = [];
let lists = [];
let shelf = { books: 0, authors: 0 };
let search = "";
let reloadToken = 0;
let coverVersion = 0;
let toastTimer = 0;
let editingId = null;
let editingTitle = "";
let editingFilePath = "";
let editingListIds = [];
let selectedBookId = null;
const authorSort = {
  key: "author",
  order: { author: "asc", count: "asc" },
};

const BOOK_COLORS = new Set([
  "#f9c650",
  "#ff8a00",
  "#fa517e",
  "#71c15d",
  "#5ea7ef",
  "#c17ad5",
  "#fce94f",
]);
const ZOOM_WIDTHS = [112, 136, 160, 192, 232, 280, 336];
const ZOOM_DEFAULT = 2;
const ZOOM_KEY = "books-zoom";
const SORT_KEY = "books-sort";
const SIDEBAR_KEY = "books-sidebar";
const BOOK_SORTS = new Set([
  "title",
  "added-desc",
  "added-asc",
  "modified-desc",
  "modified-asc",
  "rating-desc",
  "rating-asc",
]);
const SORT_ALIASES = { newest: "added-desc", oldest: "added-asc" };
let zoomLevel = ZOOM_DEFAULT;
let bookSort = "title";
const sidebarOpen = { category: true, tag: true };
const viewEdits = { zoom: false, sort: false, sidebar: false };
let viewReady = false;

const libraryNav = document.querySelector("#library-nav");
const categoryNav = document.querySelector("#category-nav");
const tagNav = document.querySelector("#tag-nav");
const listNav = document.querySelector("#list-nav");
const content = document.querySelector("#content");
const heading = document.querySelector("#heading");
const backButton = document.querySelector("#back");
const searchInput = document.querySelector("#search");
const menu = document.querySelector("#menu");
const toastElement = document.querySelector("#toast");
const bookDialog = document.querySelector("#book-dialog");
const promptDialog = document.querySelector("#prompt-dialog");
const confirmDialog = document.querySelector("#confirm-dialog");
const settingsDialog = document.querySelector("#settings-dialog");
const catalogDialog = document.querySelector("#catalog-dialog");
const catalogList = document.querySelector("#catalog-list");
const catalogStatus = document.querySelector("#catalog-status");
let catalogKind = null;
let catalogSelection = null;
let catalogBusy = false;
let catalogSortMode = "";

const CATALOGS = {
  category: {
    title: "Categories",
    empty: "No categories yet.",
    addTitle: "New category",
    addConfirm: "Add",
    addLabel: "Add category",
    renameTitle: "Rename category",
    deleteTitle: "Delete category?",
    deleteMessage: (name) => `${name} will be removed from the library and from every book.`,
    create: "create_category",
    rename: "rename_category",
    remove: "delete_category",
    move: "move_category",
    sort: "sort_category",
    color: "set_category_color",
    items: () => categories,
  },
  tag: {
    title: "Tags",
    empty: "No tags yet.",
    addTitle: "New tag",
    addConfirm: "Add",
    addLabel: "Add tag",
    renameTitle: "Rename tag",
    deleteTitle: "Delete tag?",
    deleteMessage: (name) => `${name} will be removed from the library and from every book.`,
    create: "create_tag",
    rename: "rename_tag",
    remove: "delete_tag",
    move: "move_tag",
    sort: "sort_tag",
    color: "set_tag_color",
    items: () => tags,
  },
  list: {
    title: "Lists",
    empty: "No lists yet.",
    addTitle: "New list",
    addConfirm: "Create",
    addLabel: "New list",
    renameTitle: "Rename list",
    deleteTitle: "Delete list?",
    deleteMessage: (name) => `${name} will be removed. The books stay in the library.`,
    create: "create_list",
    rename: "rename_list",
    remove: "delete_list",
    move: "move_list",
    sort: "sort_list",
    color: "set_list_color",
    items: () => lists,
  },
};

function invoke(command, args) {
  return window.__TAURI__.core.invoke(command, args);
}

function errorText(error) {
  if (typeof error === "string") return error;
  if (error && typeof error.message === "string") return error.message;
  return "Something went wrong";
}

function query() {
  return search.trim().toLowerCase();
}

function clampZoom(value) {
  const zoom = Number(value);
  if (Number.isInteger(zoom) && zoom >= 0 && zoom < ZOOM_WIDTHS.length) return zoom;
  return ZOOM_DEFAULT;
}

function canonicalSort(value) {
  const stored = SORT_ALIASES[value] || value;
  return BOOK_SORTS.has(stored) ? stored : "title";
}

function localViewSeed() {
  const seed = {};
  try {
    const zoom = localStorage.getItem(ZOOM_KEY);
    if (zoom != null) seed.zoom = clampZoom(Number(zoom));
    const sort = localStorage.getItem(SORT_KEY);
    if (sort != null) seed.bookSort = canonicalSort(sort);
    const sidebar = localStorage.getItem(SIDEBAR_KEY);
    if (sidebar != null) {
      const stored = JSON.parse(sidebar);
      seed.sidebar = {
        category: !stored || stored.category !== false,
        tag: !stored || stored.tag !== false,
      };
    }
  } catch (_error) {
    /* settings.json keeps the defaults when the old values cannot be read */
  }
  return seed;
}

function clearLocalView() {
  try {
    localStorage.removeItem(ZOOM_KEY);
    localStorage.removeItem(SORT_KEY);
    localStorage.removeItem(SIDEBAR_KEY);
  } catch (_error) {
    /* the settings file is already the copy that will be read next time */
  }
}

function applyView(settings) {
  if (!viewEdits.zoom) zoomLevel = clampZoom(settings.zoom);
  if (!viewEdits.sort) bookSort = canonicalSort(settings.bookSort);
  if (!viewEdits.sidebar) {
    const sidebar = settings.sidebar || {};
    sidebarOpen.category = sidebar.category !== false;
    sidebarOpen.tag = sidebar.tag !== false;
  }
  applyZoom();
  applySidebarSection("category");
  applySidebarSection("tag");
  const bookSortControl = document.querySelector("#book-sort");
  if (bookSortControl) bookSortControl.value = bookSort;
}

async function loadViewSettings() {
  let settings;
  try {
    settings = await invoke("prepare_settings", { seed: localViewSeed() });
  } catch (_error) {
    return;
  }
  applyView(settings);
  viewReady = true;
  if (viewEdits.zoom || viewEdits.sort || viewEdits.sidebar) await persistView();
  clearLocalView();
}

async function persistView() {
  if (!viewReady) return;
  try {
    await invoke("save_view_settings", {
      settings: {
        zoom: zoomLevel,
        bookSort,
        sidebar: {
          category: sidebarOpen.category,
          tag: sidebarOpen.tag,
        },
      },
    });
  } catch (_error) {
    /* the choice still applies for this visit */
  }
}

function sidebarNav(kind) {
  return kind === "category" ? categoryNav : tagNav;
}

function applySidebarSection(kind) {
  const open = sidebarOpen[kind];
  sidebarNav(kind).hidden = !open;
  const toggle = document.querySelector(kind === "category" ? "#toggle-categories" : "#toggle-tags");
  toggle.setAttribute("aria-expanded", open ? "true" : "false");
}

function setSidebarOpen(kind, open) {
  sidebarOpen[kind] = open;
  viewEdits.sidebar = true;
  persistView();
  applySidebarSection(kind);
}

function setBookSort(next) {
  bookSort = BOOK_SORTS.has(next) ? next : "title";
  viewEdits.sort = true;
  persistView();
  if (viewState.kind !== "authors") renderBooks();
}

function bookStamp(book) {
  const key = bookSort.startsWith("modified") ? "modifiedAt" : "addedAt";
  const value = book[key];
  return typeof value === "string" && value ? value : null;
}

function formatBookDate(value) {
  if (!value) return "Unknown";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "Unknown";
  return date.toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
}

function bookRating(book) {
  return Number.isInteger(book.rating) && book.rating >= 0 && book.rating <= 5 ? book.rating : null;
}

function compareBooks(a, b) {
  if (bookSort.startsWith("rating")) {
    const ratingA = bookRating(a);
    const ratingB = bookRating(b);
    if (ratingA == null && ratingB != null) return 1;
    if (ratingA != null && ratingB == null) return -1;
    if (ratingA != null && ratingB != null && ratingA !== ratingB) {
      return bookSort.endsWith("desc") ? ratingB - ratingA : ratingA - ratingB;
    }
  } else if (bookSort !== "title") {
    const stampA = bookStamp(a);
    const stampB = bookStamp(b);
    if (stampA == null && stampB != null) return 1;
    if (stampA != null && stampB == null) return -1;
    if (stampA != null && stampB != null && stampA !== stampB) {
      const order = stampA.localeCompare(stampB);
      return bookSort.endsWith("desc") ? -order : order;
    }
  }
  const byTitle = a.title.localeCompare(b.title, undefined, { sensitivity: "base", numeric: true });
  if (byTitle !== 0) return byTitle;
  return a.id - b.id;
}

function applyZoom() {
  document.documentElement.style.setProperty("--book-width", `${ZOOM_WIDTHS[zoomLevel]}px`);
  const zoomOut = document.querySelector("#zoom-out");
  const zoomIn = document.querySelector("#zoom-in");
  const zoomValue = document.querySelector("#zoom-level");
  if (zoomOut) zoomOut.disabled = zoomLevel === 0;
  if (zoomIn) zoomIn.disabled = zoomLevel === ZOOM_WIDTHS.length - 1;
  if (zoomValue) {
    const scale = Math.round((ZOOM_WIDTHS[zoomLevel] / ZOOM_WIDTHS[ZOOM_DEFAULT]) * 100);
    zoomValue.textContent = `${scale}%`;
  }
}

function setZoom(next) {
  zoomLevel = Math.max(0, Math.min(ZOOM_WIDTHS.length - 1, next));
  viewEdits.zoom = true;
  persistView();
  applyZoom();
}

function toast(message) {
  toastElement.textContent = message;
  toastElement.hidden = false;
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => {
    toastElement.hidden = true;
  }, 4000);
}

function hideMenu() {
  if (menu.hidden) return;
  menu.hidden = true;
  menu.replaceChildren();
  content.removeEventListener("scroll", hideMenu);
  document.querySelector("aside").removeEventListener("scroll", hideMenu);
}

function showMenu(x, y, items) {
  hideMenu();
  menu.replaceChildren();
  for (const item of items) {
    if (item.children) {
      menu.append(submenuItem(item));
      continue;
    }
    menu.append(menuButton(item));
  }
  menu.hidden = false;
  menu.style.left = "0px";
  menu.style.top = "0px";
  const rect = menu.getBoundingClientRect();
  const left = Math.min(x, window.innerWidth - rect.width - 8);
  const top = Math.min(y, window.innerHeight - rect.height - 8);
  menu.style.left = `${Math.max(8, left)}px`;
  menu.style.top = `${Math.max(8, top)}px`;
  content.addEventListener("scroll", hideMenu, { passive: true });
  document.querySelector("aside").addEventListener("scroll", hideMenu, { passive: true });
}

function menuButton(item) {
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = item.label;
  button.setAttribute("role", "menuitem");
  if (item.danger) button.className = "danger";
  if (item.disabled) {
    button.disabled = true;
    return button;
  }
  button.addEventListener("click", () => {
    hideMenu();
    item.onSelect();
  });
  return button;
}

function submenuItem(item) {
  const wrap = document.createElement("div");
  wrap.className = "menu-item";
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = item.label;
  button.setAttribute("role", "menuitem");
  button.setAttribute("aria-haspopup", "menu");
  const submenu = document.createElement("div");
  submenu.className = "submenu";
  submenu.hidden = true;
  submenu.setAttribute("role", "menu");
  for (const child of item.children) submenu.append(menuButton(child));
  const open = () => {
    submenu.hidden = false;
    const rect = submenu.getBoundingClientRect();
    submenu.classList.toggle("flip", rect.right > window.innerWidth - 8);
  };
  wrap.addEventListener("mouseenter", open);
  wrap.addEventListener("mouseleave", () => {
    submenu.hidden = true;
  });
  button.addEventListener("click", (event) => {
    event.stopPropagation();
    if (submenu.hidden) open();
    else submenu.hidden = true;
  });
  wrap.append(button, submenu);
  return wrap;
}

function coverSrc(id) {
  return `cover://localhost/${id}?v=${coverVersion}`;
}

function scopePayload() {
  if (viewState.kind === "author") return { kind: "author", author: viewState.author };
  if (viewState.kind === "category") return { kind: "category", name: viewState.name };
  if (viewState.kind === "tag") return { kind: "tag", name: viewState.name };
  if (viewState.kind === "list") return { kind: "list", id: viewState.id };
  return { kind: "all" };
}

function renderHeading() {
  const trail = headingTrail();
  heading.replaceChildren();
  if (!trail.section) {
    heading.textContent = trail.name;
    return;
  }
  const section = document.createElement("span");
  section.className = "crumb";
  section.textContent = trail.section;
  const separator = document.createElement("span");
  separator.className = "crumb-sep";
  separator.setAttribute("aria-hidden", "true");
  separator.textContent = ">";
  const current = document.createElement("span");
  current.className = "crumb-current";
  current.dir = "auto";
  current.textContent = trail.name;
  heading.append(section, separator, current);
}

function headingTrail() {
  if (viewState.kind === "authors") return { name: "Authors" };
  if (viewState.kind === "author") return { section: "Authors", name: viewState.label };
  if (viewState.kind === "category") return { section: "Categories", name: viewState.name };
  if (viewState.kind === "tag") return { section: "Tags", name: viewState.name };
  if (viewState.kind === "list") return { section: "Lists", name: viewState.name };
  return { name: "All books" };
}

async function reload() {
  const token = ++reloadToken;
  try {
    const nextLists = await invoke("lists");
    const nextShelf = await invoke("shelf_counts");
    const nextCategories = await invoke("categories");
    const nextTags = await invoke("tags");
    if (viewState.kind === "category" && !nextCategories.some((item) => item.name === viewState.name)) {
      viewState.kind = "all";
    }
    if (viewState.kind === "tag" && !nextTags.some((item) => item.name === viewState.name)) {
      viewState.kind = "all";
    }
    const nextAuthors = viewState.kind === "authors" ? await invoke("authors") : [];
    const nextBooks = viewState.kind === "authors" ? [] : await invoke("books", { scope: scopePayload() });
    if (token !== reloadToken) return;
    lists = nextLists;
    shelf = nextShelf;
    categories = nextCategories;
    tags = nextTags;
    authors = nextAuthors;
    books = nextBooks;
    if (viewState.kind === "list") {
      const current = lists.find((list) => list.id === viewState.id);
      if (current) viewState.name = current.name;
    }
    coverVersion += 1;
    render();
  } catch (error) {
    if (token !== reloadToken) return;
    toast(errorText(error));
  }
}

function render() {
  renderHeading();
  backButton.hidden = viewState.kind !== "author";
  renderLibrary();
  renderNameNav(categoryNav, categories, "category");
  renderNameNav(tagNav, tags, "tag");
  renderLists();
  content.classList.toggle("authors-view", viewState.kind === "authors");
  if (viewState.kind === "authors") renderAuthors();
  else renderBooks();
  if (catalogDialog.open) renderCatalog();
}

function renderLibrary() {
  libraryNav.replaceChildren();
  libraryNav.append(
    navButton("All books", "library", viewState.kind === "all", () => {
      viewState.kind = "all";
      reload();
    }, shelf.books),
    navButton("Authors", "author", viewState.kind === "authors" || viewState.kind === "author", () => {
      viewState.kind = "authors";
      reload();
    }, shelf.authors),
  );
}

function navButton(label, icon, active, onClick, count, color) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = active ? "nav active" : "nav";
  const main = document.createElement("span");
  main.className = "nav-main";
  main.append(navIcon(icon));
  appendCountedLabel(main, label, count, color);
  button.append(main);
  button.addEventListener("click", onClick);
  return button;
}

function appendCountedLabel(parent, label, count, color) {
  const text = document.createElement("span");
  text.className = "nav-label";
  text.textContent = label;
  parent.append(text);
  const mark = colorMark(color);
  if (mark) parent.append(mark);
  const badge = document.createElement("span");
  badge.className = "count";
  badge.textContent = `(${count})`;
  parent.append(badge);
}

function colorMark(color) {
  if (!BOOK_COLORS.has(color)) return null;
  const mark = document.createElement("span");
  mark.className = "color-mark";
  mark.style.backgroundColor = color;
  mark.setAttribute("aria-hidden", "true");
  return mark;
}

const NAV_ICON_PATHS = {
  library: "M1.35 3.15h3.7v10.45H1.35zm4.45-1.35h3.9v11.8H5.8zm4.65 2h3.7v9.8h-3.7z",
  author: "M8 3.35a2.55 2.55 0 1 1 0 5.1 2.55 2.55 0 0 1 0-5.1zM1.65 14.2c.59-3.25 2.97-5.05 6.35-5.05s5.76 1.8 6.35 5.05c.08.42-.24.85-.68.85H2.33c-.44 0-.76-.43-.68-.85z",
  list: "M2.15 4.25a1.05 1.05 0 1 0 2.1 0 1.05 1.05 0 1 0-2.1 0M5.5 3.55h8.35v1.4H5.5ZM2.15 8a1.05 1.05 0 1 0 2.1 0 1.05 1.05 0 1 0-2.1 0M5.5 7.3h8.35v1.4H5.5ZM2.15 11.75a1.05 1.05 0 1 0 2.1 0 1.05 1.05 0 1 0-2.1 0M5.5 11.05h8.35v1.4H5.5Z",
};

function navIcon(kind) {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("width", "16");
  svg.setAttribute("height", "16");
  svg.setAttribute("aria-hidden", "true");
  svg.classList.add("nav-icon");
  const add = (tag, attrs) => {
    const element = document.createElementNS("http://www.w3.org/2000/svg", tag);
    for (const [key, value] of Object.entries(attrs)) element.setAttribute(key, value);
    svg.append(element);
  };
  if (kind === "category" || kind === "tag") {
    svg.setAttribute("viewBox", "0 0 24 24");
    svg.setAttribute("fill", "none");
    svg.setAttribute("stroke", "currentColor");
    svg.setAttribute("stroke-width", "2");
    svg.setAttribute("stroke-linecap", "round");
    svg.setAttribute("stroke-linejoin", "round");
    if (kind === "category") {
      add("path", { d: "M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" });
    } else {
      add("path", { d: "M12.586 2.586A2 2 0 0 0 11.172 2H4a2 2 0 0 0-2 2v7.172a2 2 0 0 0 .586 1.414l8.704 8.704a2.426 2.426 0 0 0 3.42 0l6.58-6.58a2.426 2.426 0 0 0 0-3.42z" });
      add("path", { d: "M7.5 7.5h.01" });
    }
    return svg;
  }
  svg.setAttribute("viewBox", "0 0 16 16");
  svg.setAttribute("fill", "currentColor");
  add("path", { d: NAV_ICON_PATHS[kind] });
  return svg;
}

function renderNameNav(nav, items, kind) {
  nav.replaceChildren();
  for (const item of items) {
    nav.append(
      nameButton(item, viewState.kind === kind && viewState.name === item.name, kind, () => {
        viewState.kind = kind;
        viewState.name = item.name;
        reload();
      }),
    );
  }
}

function nameButton(item, active, kind, onClick) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = active ? "nav active" : "nav";
  const main = document.createElement("span");
  main.className = "nav-main";
  main.append(navIcon(kind));
  appendCountedLabel(main, item.name, item.count, item.color);
  button.append(main);
  button.addEventListener("click", onClick);
  return button;
}

function renderLists() {
  listNav.replaceChildren();
  for (const list of lists) {
    listNav.append(
      navButton(list.name, "list", viewState.kind === "list" && viewState.id === list.id, () => {
        viewState.kind = "list";
        viewState.id = list.id;
        viewState.name = list.name;
        reload();
      }, list.count, list.color),
    );
  }
}

function renderBooks() {
  const filtering = query().length > 0;
  const visible = books.filter((book) => bookMatches(book, query())).sort(compareBooks);
  content.replaceChildren();
  if (visible.length === 0) {
    content.append(emptyState(filtering));
    return;
  }
  const grid = document.createElement("div");
  grid.className = "grid";
  for (const book of visible) grid.append(bookTile(book));
  content.append(grid);
}

function bookMatches(book, needle) {
  if (!needle) return true;
  const notes = book.notes || "";
  return book.title.toLowerCase().includes(needle)
    || book.authorLabel.toLowerCase().includes(needle)
    || notes.toLowerCase().includes(needle);
}

function selectBook(id) {
  selectedBookId = id;
  for (const card of content.querySelectorAll(".book")) {
    const selected = Number(card.dataset.id) === id;
    card.classList.toggle("selected", selected);
    card.setAttribute("aria-selected", selected ? "true" : "false");
  }
}

function clearBookSelection() {
  selectedBookId = null;
  for (const card of content.querySelectorAll(".book.selected")) {
    card.classList.remove("selected");
    card.setAttribute("aria-selected", "false");
  }
}

function notesMark() {
  const mark = document.createElement("span");
  mark.className = "notes-mark";
  mark.setAttribute("aria-label", "Notes");
  const icon = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  icon.setAttribute("viewBox", "0 0 24 24");
  icon.setAttribute("aria-hidden", "true");
  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  path.setAttribute("fill", "currentColor");
  path.setAttribute("fill-rule", "evenodd");
  path.setAttribute("d", "M15.6729 3.91287C16.8918 2.69392 18.8682 2.69392 20.0871 3.91287C21.3061 5.13182 21.3061 7.10813 20.0871 8.32708L14.1499 14.2643C13.3849 15.0293 12.3925 15.5255 11.3215 15.6785L9.14142 15.9899C8.82983 16.0344 8.51546 15.9297 8.29289 15.7071C8.07033 15.4845 7.96554 15.1701 8.01005 14.8586L8.32149 12.6785C8.47449 11.6075 8.97072 10.615 9.7357 9.85006L15.6729 3.91287ZM18.6729 5.32708C18.235 4.88918 17.525 4.88918 17.0871 5.32708L11.1499 11.2643C10.6909 11.7233 10.3932 12.3187 10.3014 12.9613L10.1785 13.8215L11.0386 13.6986C11.6812 13.6068 12.2767 13.3091 12.7357 12.8501L18.6729 6.91287C19.1108 6.47497 19.1108 5.76499 18.6729 5.32708ZM11 3.99929C11.0004 4.55157 10.5531 4.99963 10.0008 5.00007C9.00227 5.00084 8.29769 5.00827 7.74651 5.06064C7.20685 5.11191 6.88488 5.20117 6.63803 5.32695C6.07354 5.61457 5.6146 6.07351 5.32698 6.63799C5.19279 6.90135 5.10062 7.24904 5.05118 7.8542C5.00078 8.47105 5 9.26336 5 10.4V13.6C5 14.7366 5.00078 15.5289 5.05118 16.1457C5.10062 16.7509 5.19279 17.0986 5.32698 17.3619C5.6146 17.9264 6.07354 18.3854 6.63803 18.673C6.90138 18.8072 7.24907 18.8993 7.85424 18.9488C8.47108 18.9992 9.26339 19 10.4 19H13.6C14.7366 19 15.5289 18.9992 16.1458 18.9488C16.7509 18.8993 17.0986 18.8072 17.362 18.673C17.9265 18.3854 18.3854 17.9264 18.673 17.3619C18.7988 17.1151 18.8881 16.7931 18.9393 16.2535C18.9917 15.7023 18.9991 14.9977 18.9999 13.9992C19.0003 13.4469 19.4484 12.9995 20.0007 13C20.553 13.0004 21.0003 13.4485 20.9999 14.0007C20.9991 14.9789 20.9932 15.7808 20.9304 16.4426C20.8664 17.116 20.7385 17.7136 20.455 18.2699C19.9757 19.2107 19.2108 19.9756 18.27 20.455C17.6777 20.7568 17.0375 20.8826 16.3086 20.9421C15.6008 21 14.7266 21 13.6428 21H10.3572C9.27339 21 8.39925 21 7.69138 20.9421C6.96253 20.8826 6.32234 20.7568 5.73005 20.455C4.78924 19.9756 4.02433 19.2107 3.54497 18.2699C3.24318 17.6776 3.11737 17.0374 3.05782 16.3086C2.99998 15.6007 2.99999 14.7266 3 13.6428V10.3572C2.99999 9.27337 2.99998 8.39922 3.05782 7.69134C3.11737 6.96249 3.24318 6.3223 3.54497 5.73001C4.02433 4.7892 4.78924 4.0243 5.73005 3.54493C6.28633 3.26149 6.88399 3.13358 7.55735 3.06961C8.21919 3.00673 9.02103 3.00083 9.99922 3.00007C10.5515 2.99964 10.9996 3.447 11 3.99929Z");
  icon.append(path);
  mark.append(icon);
  return mark;
}

function ratingMark(score) {
  const mark = document.createElement("span");
  mark.className = "rating-mark";
  mark.setAttribute("aria-label", `Rating ${score}`);
  const star = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  star.setAttribute("viewBox", "0 0 12 12");
  star.setAttribute("aria-hidden", "true");
  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  path.setAttribute("fill", "currentColor");
  path.setAttribute("d", "M6 .9 7.5 4.5h3.8L8.3 6.8l1 3.7L6 8.5 2.7 10.5l1-3.7L.7 4.5h3.8Z");
  star.append(path);
  const value = document.createElement("span");
  value.textContent = String(score);
  mark.append(star, value);
  return mark;
}

function bookTile(book) {
  const article = document.createElement("article");
  article.className = "book";
  article.dataset.id = String(book.id);
  article.setAttribute("aria-selected", book.id === selectedBookId ? "true" : "false");
  if (book.id === selectedBookId) article.classList.add("selected");
  article.title = `${book.title}\n${book.authorLabel}`;
  const frame = document.createElement("div");
  frame.className = "cover";
  const image = document.createElement("img");
  image.alt = book.title;
  image.src = coverSrc(book.id);
  const badge = document.createElement("span");
  badge.className = "badge";
  badge.textContent = book.formatLabel;
  if (!book.formatLabel) badge.hidden = true;
  const score = bookRating(book);
  const title = document.createElement("p");
  title.className = "book-title";
  title.dir = "auto";
  const label = document.createElement("span");
  label.textContent = book.title;
  if (BOOK_COLORS.has(book.color)) label.style.backgroundColor = book.color;
  title.append(label);
  frame.append(image, badge);
  if (book.notesPath && book.notesPath.trim()) frame.append(notesMark());
  if (score != null) frame.append(ratingMark(score));
  article.append(frame, title);
  article.addEventListener("click", () => selectBook(book.id));
  article.addEventListener("dblclick", () => openBook(book.id));
  article.addEventListener("contextmenu", (event) => {
    event.preventDefault();
    selectBook(book.id);
    const items = [
      { label: "Open", onSelect: () => openBook(book.id) },
      { label: "Open folder location", onSelect: () => openBookFolder(book.id) },
      { label: "Edit", onSelect: () => editBook(book) },
    ];
    if (viewState.kind === "list") {
      items.push({ label: "Remove from list", onSelect: () => removeFromList(book.id) });
    }
    showMenu(event.clientX, event.clientY, items);
  });
  return article;
}

function authorSortOrder() {
  return authorSort.order[authorSort.key];
}

function compareAuthors(a, b) {
  const direction = authorSortOrder() === "asc" ? 1 : -1;
  const byLabel = a.label.localeCompare(b.label, undefined, { sensitivity: "base", numeric: true });
  const primary = authorSort.key === "count" ? a.count - b.count : byLabel;
  if (primary !== 0) return primary * direction;
  return authorSort.key === "count" ? byLabel : a.count - b.count;
}

function sortMark(direction) {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("class", "sort-mark");
  svg.setAttribute("viewBox", "0 0 12 12");
  svg.setAttribute("width", "10");
  svg.setAttribute("height", "10");
  svg.setAttribute("aria-hidden", "true");
  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  path.setAttribute("fill", "currentColor");
  path.setAttribute("d", direction === "asc" ? "M6 2.2 10.2 9.2H1.8Z" : "M6 9.8 1.8 2.8h8.4Z");
  svg.append(path);
  return svg;
}

function authorColumn(key, label) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = `authors-col authors-col-${key}`;
  button.dataset.sort = key;
  const active = authorSort.key === key;
  const order = authorSort.order[key];
  button.setAttribute("aria-sort", active ? (order === "asc" ? "ascending" : "descending") : "none");
  const text = document.createElement("span");
  text.textContent = label;
  if (active && key === "count") button.append(sortMark(order));
  button.append(text);
  if (active && key !== "count") button.append(sortMark(order));
  button.addEventListener("click", () => {
    if (authorSort.key === key) authorSort.order[key] = order === "asc" ? "desc" : "asc";
    else authorSort.key = key;
    renderAuthors();
  });
  return button;
}

function authorRow(group) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "author-row";
  const name = document.createElement("span");
  name.className = "author-name";
  name.dir = "auto";
  name.textContent = group.label;
  const count = document.createElement("span");
  count.className = "author-count";
  count.textContent = String(group.count);
  button.append(name, count);
  button.addEventListener("click", () => {
    viewState.kind = "author";
    viewState.author = group.author;
    viewState.label = group.label;
    reload();
  });
  return button;
}

function renderAuthors() {
  const filtering = query().length > 0;
  const restoreSort = document.activeElement?.closest?.("[data-sort]")?.dataset.sort || "";
  const previous = content.querySelector(".authors");
  const scrollTop = previous ? previous.scrollTop : 0;
  const visible = authors.filter((group) => {
    const needle = query();
    return !needle || group.label.toLowerCase().includes(needle);
  });
  visible.sort(compareAuthors);
  content.replaceChildren();
  const list = document.createElement("div");
  list.className = "authors";
  const head = document.createElement("div");
  head.className = "authors-head";
  head.append(authorColumn("author", "Author"), authorColumn("count", "Book count"));
  const body = document.createElement("div");
  body.className = "authors-body";
  if (visible.length === 0) body.append(emptyState(filtering));
  else for (const group of visible) body.append(authorRow(group));
  list.append(head, body);
  content.append(list);
  if (scrollTop) list.scrollTop = scrollTop;
  if (restoreSort) head.querySelector(`[data-sort="${restoreSort}"]`)?.focus();
}

function emptyState(filtering) {
  const wrap = document.createElement("div");
  wrap.className = "empty";
  const title = document.createElement("h2");
  const detail = document.createElement("p");
  if (filtering) {
    title.textContent = "No matches";
    detail.textContent = viewState.kind === "authors"
      ? "Try a different title or author."
      : "Try a different title, author, or note.";
  } else {
    title.textContent = "No books";
    detail.textContent = "Add a PDF or EPUB to start your library.";
  }
  wrap.append(title, detail);
  return wrap;
}

async function openBook(id) {
  try {
    await invoke("open_book", { id });
  } catch (error) {
    toast(errorText(error));
  }
}

async function openBookFolder(id) {
  try {
    await invoke("open_book_folder", { id });
  } catch (error) {
    toast(errorText(error));
  }
}

async function removeFromList(bookId) {
  if (viewState.kind !== "list") return;
  try {
    await invoke("remove_book_from_list", { listId: viewState.id, bookId });
    await reload();
  } catch (error) {
    toast(errorText(error));
  }
}

async function removeBook(book) {
  const confirmed = await askConfirm(
    "Remove book?",
    `${book.title} will leave the library. The file on disk stays where it is.`,
    "Remove",
  );
  if (!confirmed) return false;
  try {
    await invoke("delete_book", { id: book.id });
    await reload();
    return true;
  } catch (error) {
    toast(errorText(error));
    return false;
  }
}

function editBook(book) {
  openBookDialog(book);
}

function openBookDialog(book) {
  editingId = book ? book.id : null;
  editingTitle = book ? book.title : "";
  editingFilePath = book && book.filePath ? book.filePath : "";
  editingListIds = book && book.listIds ? book.listIds.map(Number) : [];
  document.querySelector("#book-dialog-title").textContent = book ? "Edit book" : "Add book";
  document.querySelector("#book-delete").hidden = !book;
  document.querySelector("#field-file").value = "";
  document.querySelector("#field-file").placeholder = "Can be left empty, otherwise must be a valid path";
  document.querySelector("#field-cover").value = "";
  document.querySelector("#field-cover").placeholder = book
    ? "Leave empty to keep this cover"
    : "Leave empty to use the first page";
  document.querySelector("#field-notes-path").value = book && book.notesPath ? book.notesPath : "";
  document.querySelector("#field-title").value = book ? book.title : "";
  document.querySelector("#field-author").value = book ? book.author : "";
  document.querySelector("#field-year").value = book && book.year != null ? String(book.year) : "";
  document.querySelector("#field-added").closest("label").hidden = !book;
  document.querySelector("#field-modified").closest("label").hidden = !book;
  document.querySelector("#field-added").value = book ? formatBookDate(book.addedAt) : "";
  document.querySelector("#field-modified").value = book ? formatBookDate(book.modifiedAt) : "";
  document.querySelector("#field-notes").value = book && book.notes ? book.notes : "";
  selectColor(book && book.color ? book.color : "");
  selectRating(book ? book.rating : null);
  renderPicker(
    document.querySelector("#category-picker"),
    categories.map((item) => ({ value: item.name, label: item.name })),
    book ? book.categories : [],
  );
  renderPicker(
    document.querySelector("#tag-picker"),
    tags.map((item) => ({ value: item.name, label: item.name })),
    book ? book.tags : [],
  );
  renderPicker(
    document.querySelector("#list-picker"),
    lists.map((item) => ({ value: String(item.id), label: item.name })),
    editingListIds.map(String),
  );
  const frame = document.querySelector("#cover-preview-frame");
  const preview = document.querySelector("#cover-preview");
  if (book) {
    frame.hidden = false;
    preview.src = coverSrc(book.id);
    preview.alt = book.title;
  } else {
    frame.hidden = true;
    preview.removeAttribute("src");
    preview.alt = "";
  }
  syncNotesPreview();
  document.querySelector("#book-status").textContent = "";
  bookDialog.showModal();
}

function syncNotesPreview() {
  const frame = document.querySelector("#cover-preview-frame");
  const path = document.querySelector("#field-notes-path").value.trim();
  document.querySelector("#notes-preview").hidden = frame.hidden || path.length === 0;
}

async function saveBook(event) {
  event.preventDefault();
  const save = document.querySelector("#book-save");
  const status = document.querySelector("#book-status");
  save.disabled = true;
  const filePath = document.querySelector("#field-file").value;
  const draft = {
    title: document.querySelector("#field-title").value,
    author: document.querySelector("#field-author").value,
    year: document.querySelector("#field-year").value,
    filePath: filePath.trim() ? filePath : editingFilePath,
    coverPath: document.querySelector("#field-cover").value,
    notesPath: document.querySelector("#field-notes-path").value,
    categories: chosenValues(document.querySelector("#category-picker")).join(", "),
    tags: chosenValues(document.querySelector("#tag-picker")).join(", "),
    color: chosenColor(),
    rating: chosenRating(),
    notes: document.querySelector("#field-notes").value,
  };
  const listIds = chosenValues(document.querySelector("#list-picker")).map(Number);
  try {
    const saved = editingId == null
      ? await invoke("add_book", { draft })
      : await invoke("update_book", { id: editingId, draft });
    await syncLists(saved.id, listIds, editingListIds);
    bookDialog.close();
    await reload();
  } catch (error) {
    status.textContent = errorText(error);
  } finally {
    save.disabled = false;
  }
}

function renderPicker(picker, items, selected) {
  const button = picker.querySelector(".picker-button");
  const summary = button.querySelector("span");
  const menu = picker.querySelector(".picker-menu");
  menu.hidden = true;
  menu.replaceChildren();
  button.disabled = items.length === 0;
  if (items.length === 0) {
    summary.textContent = picker.dataset.empty;
    return;
  }
  const chosen = new Set(selected.map((value) => String(value).toLowerCase()));
  for (const item of items) {
    const label = document.createElement("label");
    const input = document.createElement("input");
    input.type = "checkbox";
    input.value = String(item.value);
    input.checked = chosen.has(String(item.value).toLowerCase());
    input.addEventListener("change", () => {
      summary.textContent = pickerSummary(menu);
    });
    label.append(input, document.createTextNode(item.label));
    menu.append(label);
  }
  summary.textContent = pickerSummary(menu);
}

function pickerSummary(menu) {
  const names = [...menu.querySelectorAll("input:checked")].map((input) => input.parentElement.textContent.trim());
  return names.length ? names.join(", ") : "None";
}

function chosenValues(picker) {
  return [...picker.querySelectorAll(".picker-menu input:checked")].map((input) => input.value);
}

function selectColor(color) {
  const chosen = BOOK_COLORS.has(color) ? color : "";
  for (const swatch of document.querySelectorAll("#book-colors .swatch")) {
    swatch.setAttribute("aria-pressed", swatch.dataset.color === chosen ? "true" : "false");
  }
}

function chosenColor() {
  const swatch = document.querySelector("#book-colors .swatch[aria-pressed='true']");
  return swatch ? swatch.dataset.color : "";
}

function paintStars(count) {
  for (const button of document.querySelectorAll("#book-rating .rating")) {
    button.classList.toggle("filled", count != null && Number(button.dataset.rating) <= count);
  }
}

function selectRating(rating) {
  const score = bookRating({ rating });
  const chosen = score != null && score > 0 ? score : null;
  for (const button of document.querySelectorAll("#book-rating .rating")) {
    button.setAttribute("aria-pressed", Number(button.dataset.rating) === chosen ? "true" : "false");
  }
  paintStars(chosen);
  const clear = document.querySelector("#rating-clear");
  clear.setAttribute("aria-pressed", chosen == null ? "true" : "false");
}

function chosenRating() {
  const button = document.querySelector("#book-rating .rating[aria-pressed='true']");
  return button ? button.dataset.rating : "";
}

function closePickers(except) {
  for (const menu of document.querySelectorAll(".picker-menu")) {
    if (menu !== except) menu.hidden = true;
  }
}

async function syncLists(bookId, selected, previous) {
  const next = new Set(selected);
  const previousIds = new Set(previous);
  for (const id of next) {
    if (!previousIds.has(id)) await invoke("add_book_to_list", { listId: id, bookId });
  }
  for (const id of previousIds) {
    if (!next.has(id)) await invoke("remove_book_from_list", { listId: id, bookId });
  }
}

function askPrompt(title, confirm, value) {
  return new Promise((resolve) => {
    document.querySelector("#prompt-title").textContent = title;
    document.querySelector("#prompt-ok").textContent = confirm;
    const input = document.querySelector("#prompt-input");
    input.value = value;
    const finish = (result) => {
      promptDialog.removeEventListener("close", onClose);
      resolve(result);
    };
    const onClose = () => finish(null);
    promptDialog.addEventListener("close", onClose);
    promptDialog.returnValue = "";
    promptDialog.showModal();
    input.focus();
    input.select();
    promptDialog.querySelector("#prompt-form").onsubmit = (event) => {
      event.preventDefault();
      const next = input.value;
      promptDialog.removeEventListener("close", onClose);
      promptDialog.close();
      finish(next);
    };
  });
}

function askConfirm(title, message, confirm) {
  return new Promise((resolve) => {
    document.querySelector("#confirm-title").textContent = title;
    document.querySelector("#confirm-message").textContent = message;
    document.querySelector("#confirm-ok").textContent = confirm;
    const finish = (result) => {
      confirmDialog.removeEventListener("close", onClose);
      resolve(result);
    };
    const onClose = () => finish(false);
    confirmDialog.addEventListener("close", onClose);
    confirmDialog.showModal();
    confirmDialog.querySelector("#confirm-form").onsubmit = (event) => {
      event.preventDefault();
      confirmDialog.removeEventListener("close", onClose);
      confirmDialog.close();
      finish(true);
    };
  });
}

function openCatalog(kind) {
  catalogKind = kind;
  catalogSelection = null;
  catalogSortMode = "";
  catalogStatus.textContent = "";
  renderCatalog();
  if (!catalogDialog.open) catalogDialog.showModal();
}

function renderCatalog() {
  const spec = CATALOGS[catalogKind];
  if (!spec) return;
  const scrollTop = catalogList.scrollTop;
  const items = spec.items();
  document.querySelector("#catalog-title").textContent = `${spec.title} (${items.length})`;
  document.querySelector("#catalog-add").setAttribute("aria-label", spec.addLabel);
  catalogList.replaceChildren();
  catalogList.setAttribute("role", "listbox");
  catalogList.setAttribute("aria-label", spec.title);
  if (items.length === 0) {
    const empty = document.createElement("p");
    empty.className = "catalog-empty";
    empty.textContent = spec.empty;
    catalogList.append(empty);
  }
  for (const item of items) {
    const selected = item.id === catalogSelection;
    const row = document.createElement("div");
    row.className = selected ? "catalog-row selected" : "catalog-row";
    row.dataset.id = String(item.id);
    row.setAttribute("role", "option");
    row.setAttribute("aria-selected", selected ? "true" : "false");
    const name = document.createElement("button");
    name.type = "button";
    name.className = "catalog-name";
    name.dir = "auto";
    name.append(navIcon(catalogKind));
    appendCountedLabel(name, item.name, item.count, item.color);
    name.addEventListener("click", () => selectCatalog(item.id));
    row.append(name);
    catalogList.append(row);
  }
  catalogList.scrollTop = scrollTop;
  const current = catalogList.querySelector(".catalog-row.selected");
  if (current) current.scrollIntoView({ block: "nearest" });
  updateCatalogButtons();
}

function selectCatalog(id) {
  catalogSelection = id;
  for (const row of catalogList.querySelectorAll(".catalog-row")) {
    const selected = Number(row.dataset.id) === id;
    row.classList.toggle("selected", selected);
    row.setAttribute("aria-selected", selected ? "true" : "false");
  }
  updateCatalogButtons();
}

function selectedCatalogItem() {
  const spec = CATALOGS[catalogKind];
  if (!spec || catalogSelection == null) return null;
  return spec.items().find((item) => item.id === catalogSelection) || null;
}

function sameCatalogOrder(items, sorted) {
  return sorted.every((item, index) => item.id === items[index].id);
}

function detectCatalogSort(items) {
  if (items.length < 2) return "";
  const byName = items.slice().sort((a, b) => {
    const compared = a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
    return compared || a.id - b.id;
  });
  if (sameCatalogOrder(items, byName)) return "name";
  const byNameDesc = items.slice().sort((a, b) => {
    const compared = b.name.localeCompare(a.name, undefined, { sensitivity: "base" });
    return compared || b.id - a.id;
  });
  if (sameCatalogOrder(items, byNameDesc)) return "name-desc";
  const byAdded = items.slice().sort((a, b) => a.id - b.id);
  if (sameCatalogOrder(items, byAdded)) return "added";
  return "";
}

function updateCatalogButtons() {
  const spec = CATALOGS[catalogKind];
  const items = spec ? spec.items() : [];
  const index = items.findIndex((item) => item.id === catalogSelection);
  const none = index < 0;
  document.querySelector("#catalog-up").disabled = catalogBusy || index <= 0;
  document.querySelector("#catalog-down").disabled = catalogBusy || none || index >= items.length - 1;
  document.querySelector("#catalog-rename").disabled = catalogBusy || none;
  document.querySelector("#catalog-delete").disabled = catalogBusy || none;
  const sort = document.querySelector("#catalog-sort");
  const chosen = sort.value;
  sort.disabled = catalogBusy || items.length < 2;
  sort.value = catalogBusy && chosen ? chosen : (catalogSortMode || detectCatalogSort(items));
  syncCatalogColor();
}

function syncCatalogColor() {
  const item = selectedCatalogItem();
  const chosen = item && BOOK_COLORS.has(item.color) ? item.color : "";
  for (const swatch of document.querySelectorAll("#catalog-colors .swatch")) {
    swatch.disabled = catalogBusy || !item;
    swatch.setAttribute("aria-pressed", item && swatch.dataset.color === chosen ? "true" : "false");
  }
}

async function runCatalog(task) {
  if (catalogBusy) return;
  catalogBusy = true;
  updateCatalogButtons();
  try {
    await task();
  } finally {
    catalogBusy = false;
    if (catalogDialog.open) updateCatalogButtons();
  }
}

function catalogError(error) {
  catalogStatus.textContent = errorText(error);
}

async function addCatalogItem() {
  const spec = CATALOGS[catalogKind];
  const name = await askPrompt(spec.addTitle, spec.addConfirm, "");
  if (name == null) return;
  try {
    const created = await invoke(spec.create, { name });
    catalogSelection = created.id;
    catalogSortMode = "";
    catalogStatus.textContent = "";
    await reload();
  } catch (error) {
    catalogError(error);
  }
}

async function renameCatalogItem() {
  const item = selectedCatalogItem();
  if (!item) return;
  const spec = CATALOGS[catalogKind];
  const name = await askPrompt(spec.renameTitle, "Rename", item.name);
  if (name == null) return;
  try {
    await invoke(spec.rename, { id: item.id, name });
    catalogSortMode = "";
    if (catalogKind === "list") {
      if (viewState.kind === "list" && viewState.id === item.id) viewState.name = name.trim();
    } else if (viewState.kind === catalogKind && viewState.name === item.name) {
      viewState.name = name.trim();
    }
    catalogStatus.textContent = "";
    await reload();
  } catch (error) {
    catalogError(error);
  }
}

async function removeCatalogItem() {
  const item = selectedCatalogItem();
  if (!item) return;
  const spec = CATALOGS[catalogKind];
  const confirmed = await askConfirm(spec.deleteTitle, spec.deleteMessage(item.name), "Delete");
  if (!confirmed) return;
  const items = spec.items();
  const index = items.findIndex((entry) => entry.id === item.id);
  const neighbor = items[index + 1] || items[index - 1];
  try {
    await invoke(spec.remove, { id: item.id });
    catalogSelection = neighbor && neighbor.id !== item.id ? neighbor.id : null;
    catalogSortMode = "";
    if (catalogKind === "list") {
      if (viewState.kind === "list" && viewState.id === item.id) viewState.kind = "all";
    } else if (viewState.kind === catalogKind && viewState.name === item.name) {
      viewState.kind = "all";
    }
    catalogStatus.textContent = "";
    await reload();
  } catch (error) {
    catalogError(error);
  }
}

async function moveCatalogItem(direction) {
  const spec = CATALOGS[catalogKind];
  if (catalogSelection == null) return;
  const items = spec.items();
  const index = items.findIndex((item) => item.id === catalogSelection);
  if (index < 0 || index + direction < 0 || index + direction >= items.length) return;
  try {
    await invoke(spec.move, { id: catalogSelection, direction });
    catalogSortMode = "";
    catalogStatus.textContent = "";
    await reload();
  } catch (error) {
    catalogError(error);
  }
}

async function setCatalogColor(color) {
  const item = selectedCatalogItem();
  const spec = CATALOGS[catalogKind];
  if (!item || !spec) return;
  const next = BOOK_COLORS.has(color) ? color : "";
  if ((item.color || "") === next) return;
  try {
    await invoke(spec.color, { id: item.id, color: next });
    catalogStatus.textContent = "";
    await reload();
  } catch (error) {
    catalogError(error);
  }
}

async function sortCatalogItems(mode) {
  const spec = CATALOGS[catalogKind];
  if (!spec || !mode) return;
  try {
    await invoke(spec.sort, { mode });
    catalogSortMode = mode;
    catalogStatus.textContent = "";
    await reload();
  } catch (error) {
    catalogError(error);
  }
}

async function openSettings() {
  const database = document.querySelector("#settings-database");
  const covers = document.querySelector("#settings-covers");
  const settingsFile = document.querySelector("#settings-file");
  const reader = document.querySelector("#settings-reader");
  const fileManager = document.querySelector("#settings-file-manager");
  const status = document.querySelector("#settings-status");
  database.value = "";
  covers.value = "";
  settingsFile.value = "";
  reader.value = "";
  fileManager.value = "";
  status.textContent = "";
  settingsDialog.showModal();
  try {
    const settings = await invoke("app_settings");
    database.value = settings.databasePath;
    covers.value = settings.coversPath;
    settingsFile.value = settings.settingsPath;
    reader.value = settings.reader;
    fileManager.value = settings.fileManager;
  } catch (error) {
    status.textContent = errorText(error);
  }
}

async function saveSettings() {
  const status = document.querySelector("#settings-status");
  status.textContent = "";
  try {
    await invoke("save_app_settings", {
      settings: {
        reader: document.querySelector("#settings-reader").value,
        fileManager: document.querySelector("#settings-file-manager").value,
      },
    });
    settingsDialog.close();
  } catch (error) {
    status.textContent = errorText(error);
  }
}

function bindSidebarScrollbar() {
  const aside = document.querySelector("aside");
  const scroller = aside.querySelector(".sidebar-scroll");
  let refreshing = false;
  new ResizeObserver(() => {
    if (refreshing) return;
    refreshing = true;
    const top = scroller.scrollTop;
    scroller.style.overflowY = "hidden";
    void scroller.offsetHeight;
    scroller.style.overflowY = "";
    scroller.scrollTop = top;
    refreshing = false;
  }).observe(aside);
}

function bind() {
  applyZoom();
  applySidebarSection("category");
  applySidebarSection("tag");
  bindSidebarScrollbar();
  document.querySelector("#toggle-categories").addEventListener("click", () => {
    setSidebarOpen("category", !sidebarOpen.category);
  });
  document.querySelector("#toggle-tags").addEventListener("click", () => {
    setSidebarOpen("tag", !sidebarOpen.tag);
  });
  const bookSortControl = document.querySelector("#book-sort");
  bookSortControl.value = bookSort;
  bookSortControl.addEventListener("change", () => setBookSort(bookSortControl.value));
  document.querySelector("#zoom-out").addEventListener("click", () => setZoom(zoomLevel - 1));
  document.querySelector("#zoom-in").addEventListener("click", () => setZoom(zoomLevel + 1));
  document.querySelector("#manage-categories").addEventListener("click", () => openCatalog("category"));
  document.querySelector("#manage-tags").addEventListener("click", () => openCatalog("tag"));
  document.querySelector("#manage-lists").addEventListener("click", () => openCatalog("list"));
  document.querySelector("#catalog-add").addEventListener("click", () => {
    runCatalog(() => addCatalogItem()).catch(catalogError);
  });
  document.querySelector("#catalog-up").addEventListener("click", () => {
    runCatalog(() => moveCatalogItem(-1)).catch(catalogError);
  });
  document.querySelector("#catalog-down").addEventListener("click", () => {
    runCatalog(() => moveCatalogItem(1)).catch(catalogError);
  });
  document.querySelector("#catalog-sort").addEventListener("change", () => {
    const mode = document.querySelector("#catalog-sort").value;
    if (!mode) return;
    runCatalog(() => sortCatalogItems(mode)).catch(catalogError);
  });
  document.querySelector("#catalog-rename").addEventListener("click", () => {
    runCatalog(() => renameCatalogItem()).catch(catalogError);
  });
  document.querySelector("#catalog-delete").addEventListener("click", () => {
    runCatalog(() => removeCatalogItem()).catch(catalogError);
  });
  document.querySelector("#catalog-close").addEventListener("click", () => catalogDialog.close());
  for (const swatch of document.querySelectorAll("#catalog-colors .swatch")) {
    swatch.addEventListener("click", () => {
      runCatalog(() => setCatalogColor(swatch.dataset.color)).catch(catalogError);
    });
  }
  document.querySelector("#catalog-form").addEventListener("submit", (event) => {
    event.preventDefault();
  });
  document.querySelector("#add-book").addEventListener("click", () => openBookDialog(null));
  for (const swatch of document.querySelectorAll("#book-colors .swatch")) {
    swatch.addEventListener("click", () => selectColor(swatch.dataset.color));
  }
  document.querySelector("#rating-clear").addEventListener("click", () => selectRating(null));
  for (const button of document.querySelectorAll("#book-rating .rating")) {
    button.addEventListener("click", () => {
      const value = Number(button.dataset.rating);
      selectRating(chosenRating() === String(value) ? null : value);
    });
  }
  for (const picker of document.querySelectorAll(".picker")) {
    const button = picker.querySelector(".picker-button");
    const menu = picker.querySelector(".picker-menu");
    button.addEventListener("click", (event) => {
      event.stopPropagation();
      if (button.disabled) return;
      const willOpen = menu.hidden;
      closePickers();
      menu.hidden = !willOpen;
    });
  }
  document.addEventListener("click", (event) => {
    if (!event.target.closest(".picker")) closePickers();
  });
  document.querySelector("#field-notes-path").addEventListener("input", syncNotesPreview);
  document.querySelector("#book-form").addEventListener("submit", (event) => {
    saveBook(event).catch((error) => {
      document.querySelector("#book-status").textContent = errorText(error);
    });
  });
  document.querySelector("#book-cancel").addEventListener("click", () => bookDialog.close());
  document.querySelector("#book-delete").addEventListener("click", () => {
    const title = document.querySelector("#field-title").value.trim() || editingTitle || "This book";
    removeBook({ id: editingId, title }).then((removed) => {
      if (removed) bookDialog.close();
    });
  });
  document.querySelector("#prompt-cancel").addEventListener("click", () => promptDialog.close());
  document.querySelector("#confirm-cancel").addEventListener("click", () => confirmDialog.close());
  document.querySelector("#settings").addEventListener("click", () => {
    openSettings().catch((error) => {
      document.querySelector("#settings-status").textContent = errorText(error);
    });
  });
  document.querySelector("#settings-cancel").addEventListener("click", () => settingsDialog.close());
  document.querySelector("#settings-close").addEventListener("click", () => {
    saveSettings().catch((error) => {
      document.querySelector("#settings-status").textContent = errorText(error);
    });
  });
  document.querySelector("#settings-form").addEventListener("submit", (event) => {
    event.preventDefault();
    saveSettings().catch((error) => {
      document.querySelector("#settings-status").textContent = errorText(error);
    });
  });
  backButton.addEventListener("click", () => {
    viewState.kind = "authors";
    reload();
  });
  searchInput.addEventListener("input", () => {
    search = searchInput.value;
    render();
  });
  window.addEventListener("click", (event) => {
    if (!menu.hidden && !menu.contains(event.target)) hideMenu();
  });
  window.addEventListener("keydown", (event) => {
    if (event.key === "Escape") hideMenu();
    if (!(event.ctrlKey || event.metaKey) || event.altKey) return;
    if (event.target.closest("input, textarea") || document.querySelector("dialog[open]")) return;
    if (event.key === "=" || event.key === "+") {
      event.preventDefault();
      setZoom(zoomLevel + 1);
    } else if (event.key === "-" || event.key === "_") {
      event.preventDefault();
      setZoom(zoomLevel - 1);
    } else if (event.key === "0") {
      event.preventDefault();
      setZoom(ZOOM_DEFAULT);
    }
  });
  content.addEventListener("click", (event) => {
    if (!event.target.closest(".book")) clearBookSelection();
  });
}

bind();
if (!window.__TAURI__?.core?.invoke) {
  content.append(emptyState(false));
  content.querySelector("h2").textContent = "Open Books from its desktop window";
  content.querySelector("p").textContent = "The shelf talks to the local catalog from there.";
} else {
  loadViewSettings().then(() => reload());
}
