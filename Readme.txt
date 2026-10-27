#Similar apps:
1. https://github.com/everydaythingssoftware/citadel
2. Apple Books

# TODO:
implement an app for linux desktop to organize pdf and epub books

I want to be able to add a new book to the database: Has title, author, year, categories, tags, front cover and a link to a local file. If the front cover isn't given, it should be extracted from the first page of the pdf/epub.

The books should be displayed in a grid, just like Apple Books.

On the left side is a side panel that has options: Library section containing "All books" and "Authors". And a "Lists" section, where I can create new lists and drag and drop books from the library.

Double clicking a book should open the book using Okular (or a different app).

A small icon to the bottom right of a book cover should show if it's a pdf or an epub.

Use Tauri + HTML/CSS

***

The add book dialog should not open a window to browse files

Instead: It should ask for 2 paths: One for the book; and another for the cover.

If the cover path isn't supplied, it is gotten from the first page.

It is possible to add a book entry without a book path or a cover path.

***

The side panel should have 2 more sections: One is "Categories" another is "Tags" each one to browse all existing categories and tags across all books.



