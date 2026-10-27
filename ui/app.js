const viewState = { kind: "all" };
let books = [];
let authors = [];
let categories = [];
let tags = [];
let lists = [];
let search = "";
let reloadToken = 0;
let coverVersion = 0;
let toastTimer = 0;
let editingId = null;
let editingTitle = "";
let editingListIds = [];
let selectedBookId = null;

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
const ZOOM_KEY = "books-zoom";
let zoomLevel = storedZoom();

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

function storedZoom() {
  try {
    const raw = localStorage.getItem(ZOOM_KEY);
    if (raw == null) return 2;
    const stored = Number(raw);
    if (Number.isInteger(stored) && stored >= 0 && stored < ZOOM_WIDTHS.length) return stored;
  } catch (_error) {
    /* keep the default size when storage is unavailable */
  }
  return 2;
}

function applyZoom() {
  document.documentElement.style.setProperty("--book-width", `${ZOOM_WIDTHS[zoomLevel]}px`);
  const zoomOut = document.querySelector("#zoom-out");
  const zoomIn = document.querySelector("#zoom-in");
  if (zoomOut) zoomOut.disabled = zoomLevel === 0;
  if (zoomIn) zoomIn.disabled = zoomLevel === ZOOM_WIDTHS.length - 1;
}

function setZoom(next) {
  zoomLevel = Math.max(0, Math.min(ZOOM_WIDTHS.length - 1, next));
  try {
    localStorage.setItem(ZOOM_KEY, String(zoomLevel));
  } catch (_error) {
    /* the chosen size still applies for this visit */
  }
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

function headingText() {
  if (viewState.kind === "authors") return "Authors";
  if (viewState.kind === "author") return viewState.label;
  if (viewState.kind === "category" || viewState.kind === "tag" || viewState.kind === "list") {
    return viewState.name;
  }
  return "All books";
}

async function reload() {
  const token = ++reloadToken;
  try {
    const nextLists = await invoke("lists");
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
  heading.textContent = headingText();
  backButton.hidden = viewState.kind !== "author";
  renderLibrary();
  renderNameNav(categoryNav, categories, "category");
  renderNameNav(tagNav, tags, "tag");
  renderLists();
  if (viewState.kind === "authors") renderAuthors();
  else renderBooks();
}

function renderLibrary() {
  libraryNav.replaceChildren();
  libraryNav.append(
    navButton("All books", viewState.kind === "all", () => {
      viewState.kind = "all";
      reload();
    }),
    navButton("Authors", viewState.kind === "authors" || viewState.kind === "author", () => {
      viewState.kind = "authors";
      reload();
    }),
  );
}

function navButton(label, active, onClick) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = active ? "nav active" : "nav";
  button.textContent = label;
  button.addEventListener("click", onClick);
  return button;
}

function renderNameNav(nav, items, kind) {
  nav.replaceChildren();
  for (const item of items) {
    nav.append(
      nameButton(item, viewState.kind === kind && viewState.name === item.name, () => {
        viewState.kind = kind;
        viewState.name = item.name;
        reload();
      }),
    );
  }
}

function nameButton(item, active, onClick) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = active ? "nav active" : "nav";
  const name = document.createElement("span");
  name.textContent = item.name;
  const count = document.createElement("span");
  count.className = "count";
  count.textContent = String(item.count);
  button.append(name, count);
  button.addEventListener("click", onClick);
  return button;
}

function renderLists() {
  listNav.replaceChildren();
  for (const list of lists) {
    const row = document.createElement("div");
    row.className = viewState.kind === "list" && viewState.id === list.id ? "list-row active" : "list-row";
    const name = document.createElement("button");
    name.type = "button";
    name.className = "list-name";
    name.textContent = list.name;
    name.addEventListener("click", () => {
      viewState.kind = "list";
      viewState.id = list.id;
      viewState.name = list.name;
      reload();
    });
    const more = document.createElement("button");
    more.type = "button";
    more.className = "list-more";
    more.setAttribute("aria-label", "List actions");
    more.textContent = "···";
    more.addEventListener("click", (event) => {
      event.stopPropagation();
      const rect = more.getBoundingClientRect();
      showMenu(rect.left, rect.bottom, [
        { label: "Rename", onSelect: () => renameList(list) },
        { label: "Delete", danger: true, onSelect: () => removeList(list) },
      ]);
    });
    row.append(name, more);
    listNav.append(row);
  }
}

function renderBooks() {
  const filtering = query().length > 0;
  const visible = books.filter((book) => bookMatches(book, query()));
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
  return book.title.toLowerCase().includes(needle) || book.authorLabel.toLowerCase().includes(needle);
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
  const title = document.createElement("p");
  title.className = "book-title";
  title.dir = "auto";
  const label = document.createElement("span");
  label.textContent = book.title;
  if (BOOK_COLORS.has(book.color)) label.style.backgroundColor = book.color;
  title.append(label);
  frame.append(image, badge);
  article.append(frame, title);
  article.addEventListener("click", () => selectBook(book.id));
  article.addEventListener("dblclick", () => openBook(book.id));
  article.addEventListener("contextmenu", (event) => {
    event.preventDefault();
    selectBook(book.id);
    const items = [
      { label: "Open", onSelect: () => openBook(book.id) },
      { label: "Edit", onSelect: () => editBook(book) },
    ];
    if (viewState.kind === "list") {
      items.push({ label: "Remove from list", onSelect: () => removeFromList(book.id) });
    }
    showMenu(event.clientX, event.clientY, items);
  });
  return article;
}

function renderAuthors() {
  const filtering = query().length > 0;
  const visible = authors.filter((group) => {
    const needle = query();
    return !needle || group.label.toLowerCase().includes(needle);
  });
  content.replaceChildren();
  if (visible.length === 0) {
    content.append(emptyState(filtering));
    return;
  }
  const list = document.createElement("div");
  list.className = "authors";
  for (const group of visible) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "author-row";
    const name = document.createElement("span");
    name.textContent = group.label;
    const count = document.createElement("span");
    count.className = "count";
    count.textContent = String(group.count);
    button.append(name, count);
    button.addEventListener("click", () => {
      viewState.kind = "author";
      viewState.author = group.author;
      viewState.label = group.label;
      reload();
    });
    list.append(button);
  }
  content.append(list);
}

function emptyState(filtering) {
  const wrap = document.createElement("div");
  wrap.className = "empty";
  const title = document.createElement("h2");
  const detail = document.createElement("p");
  if (filtering) {
    title.textContent = "No matches";
    detail.textContent = "Try a different title or author.";
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
  editingListIds = book && book.listIds ? book.listIds.map(Number) : [];
  document.querySelector("#book-dialog-title").textContent = book ? "Edit book" : "Add book";
  document.querySelector("#book-delete").hidden = !book;
  document.querySelector("#field-file").value = book ? book.filePath : "";
  document.querySelector("#field-cover").value = "";
  document.querySelector("#field-cover").placeholder = book
    ? "Leave empty to keep this cover"
    : "Leave empty to use the first page";
  document.querySelector("#field-title").value = book ? book.title : "";
  document.querySelector("#field-author").value = book ? book.author : "";
  document.querySelector("#field-year").value = book && book.year != null ? String(book.year) : "";
  document.querySelector("#field-notes").value = book && book.notes ? book.notes : "";
  selectColor(book && book.color ? book.color : "");
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
  const preview = document.querySelector("#cover-preview");
  if (book) {
    preview.hidden = false;
    preview.src = coverSrc(book.id);
    preview.alt = book.title;
  } else {
    preview.hidden = true;
    preview.removeAttribute("src");
    preview.alt = "";
  }
  document.querySelector("#book-status").textContent = "";
  bookDialog.showModal();
}

async function saveBook(event) {
  event.preventDefault();
  const save = document.querySelector("#book-save");
  const status = document.querySelector("#book-status");
  save.disabled = true;
  const draft = {
    title: document.querySelector("#field-title").value,
    author: document.querySelector("#field-author").value,
    year: document.querySelector("#field-year").value,
    filePath: document.querySelector("#field-file").value,
    coverPath: document.querySelector("#field-cover").value,
    categories: chosenValues(document.querySelector("#category-picker")).join(", "),
    tags: chosenValues(document.querySelector("#tag-picker")).join(", "),
    color: chosenColor(),
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

async function addCatalogName(title, confirm, command) {
  const name = await askPrompt(title, confirm, "");
  if (name == null) return;
  try {
    await invoke(command, { name });
    await reload();
  } catch (error) {
    toast(errorText(error));
  }
}

async function renameList(list) {
  const name = await askPrompt("Rename list", "Rename", list.name);
  if (name == null) return;
  try {
    await invoke("rename_list", { id: list.id, name });
    if (viewState.kind === "list" && viewState.id === list.id) viewState.name = name.trim();
    await reload();
  } catch (error) {
    toast(errorText(error));
  }
}

async function removeList(list) {
  const confirmed = await askConfirm(
    "Delete list?",
    `${list.name} will be removed. The books stay in the library.`,
    "Delete",
  );
  if (!confirmed) return;
  try {
    await invoke("delete_list", { id: list.id });
    if (viewState.kind === "list" && viewState.id === list.id) viewState.kind = "all";
    await reload();
  } catch (error) {
    toast(errorText(error));
  }
}

function bind() {
  applyZoom();
  document.querySelector("#zoom-out").addEventListener("click", () => setZoom(zoomLevel - 1));
  document.querySelector("#zoom-in").addEventListener("click", () => setZoom(zoomLevel + 1));
  document.querySelector("#new-list").addEventListener("click", async () => {
    const name = await askPrompt("New list", "Create", "");
    if (name == null) return;
    try {
      const list = await invoke("create_list", { name });
      viewState.kind = "list";
      viewState.id = list.id;
      viewState.name = list.name;
      await reload();
    } catch (error) {
      toast(errorText(error));
    }
  });
  document.querySelector("#new-category").addEventListener("click", () => {
    addCatalogName("New category", "Add", "create_category");
  });
  document.querySelector("#new-tag").addEventListener("click", () => {
    addCatalogName("New tag", "Add", "create_tag");
  });
  document.querySelector("#add-book").addEventListener("click", () => openBookDialog(null));
  for (const swatch of document.querySelectorAll("#book-colors .swatch")) {
    swatch.addEventListener("click", () => selectColor(swatch.dataset.color));
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
      setZoom(2);
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
  reload();
}
