const BOOK_DRAG = "application/x-book-id";

const viewState = { kind: "all" };
let books = [];
let authors = [];
let lists = [];
let search = "";
let reloadToken = 0;
let coverVersion = 0;
let toastTimer = 0;
let fileGeneration = 0;
let editingId = null;
let draftPath = "";
let draftCover = "";

const libraryNav = document.querySelector("#library-nav");
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

function toast(message) {
  toastElement.textContent = message;
  toastElement.hidden = false;
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => {
    toastElement.hidden = true;
  }, 4000);
}

function hideMenu() {
  menu.hidden = true;
  menu.replaceChildren();
}

function showMenu(x, y, items) {
  menu.replaceChildren();
  for (const item of items) {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = item.label;
    button.setAttribute("role", "menuitem");
    if (item.danger) button.className = "danger";
    button.addEventListener("click", () => {
      hideMenu();
      item.onSelect();
    });
    menu.append(button);
  }
  menu.hidden = false;
  menu.style.left = "0px";
  menu.style.top = "0px";
  const rect = menu.getBoundingClientRect();
  const left = Math.min(x, window.innerWidth - rect.width - 8);
  const top = Math.min(y, window.innerHeight - rect.height - 8);
  menu.style.left = `${Math.max(8, left)}px`;
  menu.style.top = `${Math.max(8, top)}px`;
}

function coverSrc(id) {
  return `cover://localhost/${id}?v=${coverVersion}`;
}

function scopePayload() {
  if (viewState.kind === "author") return { kind: "author", author: viewState.author };
  if (viewState.kind === "list") return { kind: "list", id: viewState.id };
  return { kind: "all" };
}

function headingText() {
  if (viewState.kind === "authors") return "Authors";
  if (viewState.kind === "author") return viewState.label;
  if (viewState.kind === "list") return viewState.name;
  return "All books";
}

async function reload() {
  const token = ++reloadToken;
  try {
    const nextLists = await invoke("lists");
    const nextAuthors = viewState.kind === "authors" ? await invoke("authors") : [];
    const nextBooks = viewState.kind === "authors" ? [] : await invoke("books", { scope: scopePayload() });
    if (token !== reloadToken) return;
    lists = nextLists;
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
    row.addEventListener("dragover", (event) => {
      if (!isBookDrag(event)) return;
      event.preventDefault();
      row.classList.add("drop");
    });
    row.addEventListener("dragleave", () => row.classList.remove("drop"));
    row.addEventListener("drop", (event) => {
      row.classList.remove("drop");
      event.preventDefault();
      const bookId = droppedBookId(event);
      if (bookId) dropOnList(list, bookId);
    });
    listNav.append(row);
  }
}

function isBookDrag(event) {
  return Array.from(event.dataTransfer.types).includes(BOOK_DRAG);
}

function droppedBookId(event) {
  const raw = event.dataTransfer.getData(BOOK_DRAG) || event.dataTransfer.getData("text/plain");
  const bookId = Number(raw);
  return Number.isInteger(bookId) && bookId > 0 ? bookId : 0;
}

async function dropOnList(list, bookId) {
  try {
    const added = await invoke("add_book_to_list", { listId: list.id, bookId });
    toast(added ? `Added to ${list.name}` : `Already in ${list.name}`);
    await reload();
  } catch (error) {
    toast(errorText(error));
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

function bookTile(book) {
  const article = document.createElement("article");
  article.className = "book";
  article.draggable = true;
  article.title = `${book.title}\n${book.authorLabel}`;
  const frame = document.createElement("div");
  frame.className = "cover";
  const image = document.createElement("img");
  image.draggable = false;
  image.alt = book.title;
  image.src = coverSrc(book.id);
  const badge = document.createElement("span");
  badge.className = "badge";
  badge.textContent = book.formatLabel;
  const title = document.createElement("p");
  title.className = "book-title";
  title.textContent = book.title;
  frame.append(image, badge);
  article.append(frame, title);
  article.addEventListener("dragstart", (event) => {
    event.dataTransfer.setData(BOOK_DRAG, String(book.id));
    event.dataTransfer.setData("text/plain", String(book.id));
    event.dataTransfer.effectAllowed = "copy";
  });
  article.addEventListener("dblclick", () => openBook(book.id));
  article.addEventListener("contextmenu", (event) => {
    event.preventDefault();
    const items = [
      { label: "Open", onSelect: () => openBook(book.id) },
      { label: "Edit", onSelect: () => editBook(book) },
    ];
    if (viewState.kind === "list") {
      items.push({ label: "Remove from list", onSelect: () => removeFromList(book.id) });
    }
    items.push({ label: "Remove from library", danger: true, onSelect: () => removeBook(book) });
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
  if (!confirmed) return;
  try {
    await invoke("delete_book", { id: book.id });
    await reload();
  } catch (error) {
    toast(errorText(error));
  }
}

async function editBook(book) {
  let cover = "";
  try {
    const response = await fetch(coverSrc(book.id));
    if (response.ok) cover = await bytesToBase64(new Uint8Array(await response.arrayBuffer()));
  } catch (_error) {
    cover = "";
  }
  openBookDialog(book, cover);
}

function openBookDialog(book, cover) {
  editingId = book ? book.id : null;
  draftPath = book ? book.filePath : "";
  draftCover = cover || "";
  fileGeneration += 1;
  document.querySelector("#book-dialog-title").textContent = book ? "Edit book" : "Add book";
  document.querySelector("#field-title").value = book ? book.title : "";
  document.querySelector("#field-author").value = book ? book.author : "";
  document.querySelector("#field-year").value = book && book.year != null ? String(book.year) : "";
  document.querySelector("#field-categories").value = book ? book.categories.join(", ") : "";
  document.querySelector("#field-tags").value = book ? book.tags.join(", ") : "";
  const fileLabel = document.querySelector("#file-label");
  fileLabel.textContent = draftPath || "No file chosen";
  fileLabel.title = draftPath;
  const preview = document.querySelector("#cover-preview");
  preview.src = draftCover ? `data:image/png;base64,${draftCover}` : "";
  document.querySelector("#book-status").textContent = "";
  document.querySelector("#book-save").disabled = !draftPath;
  bookDialog.showModal();
}

async function chooseBookFile() {
  const path = await invoke("pick_book");
  if (!path) return;
  const generation = ++fileGeneration;
  const status = document.querySelector("#book-status");
  const save = document.querySelector("#book-save");
  status.textContent = "Reading the file…";
  save.disabled = true;
  try {
    const inspected = await invoke("inspect_file", { path });
    if (generation !== fileGeneration) return;
    draftPath = path;
    draftCover = inspected.coverPng;
    document.querySelector("#field-title").value = inspected.title;
    document.querySelector("#field-author").value = inspected.author;
    document.querySelector("#field-year").value = inspected.year == null ? "" : String(inspected.year);
    const fileLabel = document.querySelector("#file-label");
    fileLabel.textContent = path;
    fileLabel.title = path;
    document.querySelector("#cover-preview").src = `data:image/png;base64,${draftCover}`;
    status.textContent = "";
  } catch (error) {
    if (generation !== fileGeneration) return;
    status.textContent = errorText(error);
  } finally {
    if (generation === fileGeneration) save.disabled = !draftPath;
  }
}

async function chooseCoverFile() {
  const path = await invoke("pick_cover");
  if (!path) return;
  const status = document.querySelector("#book-status");
  try {
    draftCover = await invoke("cover_from_image", { path });
    document.querySelector("#cover-preview").src = `data:image/png;base64,${draftCover}`;
    status.textContent = "";
  } catch (error) {
    status.textContent = errorText(error);
  }
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
    filePath: draftPath,
    categories: document.querySelector("#field-categories").value,
    tags: document.querySelector("#field-tags").value,
    coverPng: draftCover,
  };
  try {
    if (editingId == null) await invoke("add_book", { draft });
    else await invoke("update_book", { id: editingId, draft });
    bookDialog.close();
    await reload();
  } catch (error) {
    status.textContent = errorText(error);
    save.disabled = !draftPath;
  }
}

async function bytesToBase64(bytes) {
  let binary = "";
  const chunk = 0x8000;
  for (let index = 0; index < bytes.length; index += chunk) {
    binary += String.fromCharCode(...bytes.subarray(index, index + chunk));
  }
  return btoa(binary);
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
  document.querySelector("#add-book").addEventListener("click", () => openBookDialog(null, ""));
  document.querySelector("#choose-file").addEventListener("click", () => {
    chooseBookFile().catch((error) => {
      document.querySelector("#book-status").textContent = errorText(error);
    });
  });
  document.querySelector("#choose-cover").addEventListener("click", () => {
    chooseCoverFile().catch((error) => {
      document.querySelector("#book-status").textContent = errorText(error);
    });
  });
  document.querySelector("#book-form").addEventListener("submit", (event) => {
    saveBook(event).catch((error) => {
      document.querySelector("#book-status").textContent = errorText(error);
    });
  });
  document.querySelector("#book-cancel").addEventListener("click", () => bookDialog.close());
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
  });
  content.addEventListener("scroll", hideMenu);
}

bind();
if (!window.__TAURI__?.core?.invoke) {
  content.append(emptyState(false));
  content.querySelector("h2").textContent = "Open Books from its desktop window";
  content.querySelector("p").textContent = "The shelf talks to the local catalog from there.";
} else {
  reload();
}
