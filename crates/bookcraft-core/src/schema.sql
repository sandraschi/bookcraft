        CREATE TABLE books (id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL DEFAULT 'Unknown' COLLATE NOCASE,
            sort TEXT COLLATE NOCASE,
            timestamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            pubdate TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            series_index REAL NOT NULL DEFAULT 1.0,
            author_sort TEXT COLLATE NOCASE,
            path TEXT NOT NULL DEFAULT '',
            uuid TEXT,
            has_cover BOOL DEFAULT 0,
            last_modified TIMESTAMP NOT NULL DEFAULT '2000-01-01 00:00:00+00:00');
        CREATE TABLE authors (id INTEGER PRIMARY KEY,
            name TEXT NOT NULL COLLATE NOCASE, sort TEXT COLLATE NOCASE,
            link TEXT NOT NULL DEFAULT '', UNIQUE(name));
        CREATE TABLE tags (id INTEGER PRIMARY KEY,
            name TEXT NOT NULL COLLATE NOCASE, link TEXT NOT NULL DEFAULT '',
            UNIQUE(name));
        CREATE TABLE series (id INTEGER PRIMARY KEY,
            name TEXT NOT NULL COLLATE NOCASE, sort TEXT COLLATE NOCASE,
            link TEXT NOT NULL DEFAULT '', UNIQUE(name));
        CREATE TABLE publishers (id INTEGER PRIMARY KEY,
            name TEXT NOT NULL COLLATE NOCASE, sort TEXT COLLATE NOCASE,
            link TEXT NOT NULL DEFAULT '', UNIQUE(name));
        CREATE TABLE ratings (id INTEGER PRIMARY KEY,
            rating INTEGER CHECK(rating > -1 AND rating < 11),
            link TEXT NOT NULL DEFAULT '', UNIQUE(rating));
        CREATE TABLE languages (id INTEGER PRIMARY KEY,
            lang_code TEXT NOT NULL COLLATE NOCASE,
            link TEXT NOT NULL DEFAULT '', UNIQUE(lang_code));
        CREATE TABLE identifiers (id INTEGER PRIMARY KEY, book INTEGER NOT NULL,
            type TEXT NOT NULL DEFAULT 'isbn' COLLATE NOCASE,
            val TEXT NOT NULL COLLATE NOCASE, UNIQUE(book, type));
        CREATE TABLE comments (id INTEGER PRIMARY KEY, book INTEGER NOT NULL,
            text TEXT NOT NULL COLLATE NOCASE, UNIQUE(book));
        CREATE TABLE data (id INTEGER PRIMARY KEY, book INTEGER NOT NULL,
            format TEXT NOT NULL COLLATE NOCASE, uncompressed_size INTEGER NOT NULL,
            name TEXT NOT NULL, UNIQUE(book, format));
        CREATE TABLE books_authors_link (id INTEGER PRIMARY KEY,
            book INTEGER NOT NULL, author INTEGER NOT NULL, UNIQUE(book, author));
        CREATE TABLE books_tags_link (id INTEGER PRIMARY KEY,
            book INTEGER NOT NULL, tag INTEGER NOT NULL, UNIQUE(book, tag));
        CREATE TABLE books_series_link (id INTEGER PRIMARY KEY,
            book INTEGER NOT NULL, series INTEGER NOT NULL, UNIQUE(book));
        CREATE TABLE books_languages_link (id INTEGER PRIMARY KEY,
            book INTEGER NOT NULL, lang_code INTEGER NOT NULL,
            item_order INTEGER NOT NULL DEFAULT 0, UNIQUE(book, lang_code));
        CREATE TABLE books_publishers_link (id INTEGER PRIMARY KEY,
            book INTEGER NOT NULL, publisher INTEGER NOT NULL, UNIQUE(book));
        CREATE TABLE books_ratings_link (id INTEGER PRIMARY KEY,
            book INTEGER NOT NULL, rating INTEGER NOT NULL, UNIQUE(book, rating));
        CREATE TABLE metadata_dirtied(id INTEGER PRIMARY KEY,
            book INTEGER NOT NULL, UNIQUE(book));
        CREATE TABLE last_read_positions (book INTEGER);
        CREATE TABLE annotations (book INTEGER);
        CREATE TABLE conversion_options (book INTEGER);
        CREATE TABLE books_plugin_data (book INTEGER);
        CREATE TRIGGER books_insert_trg AFTER INSERT ON books BEGIN
            UPDATE books SET sort=title_sort(NEW.title),uuid=uuid4() WHERE id=NEW.id;
        END;
        CREATE TRIGGER books_update_trg AFTER UPDATE ON books BEGIN
            UPDATE books SET sort=title_sort(NEW.title)
            WHERE id=NEW.id AND OLD.title <> NEW.title;
        END;
        CREATE TRIGGER books_delete_trg AFTER DELETE ON books BEGIN
            DELETE FROM books_authors_link WHERE book=OLD.id;
            DELETE FROM books_publishers_link WHERE book=OLD.id;
            DELETE FROM books_ratings_link WHERE book=OLD.id;
            DELETE FROM books_series_link WHERE book=OLD.id;
            DELETE FROM books_tags_link WHERE book=OLD.id;
            DELETE FROM books_languages_link WHERE book=OLD.id;
            DELETE FROM data WHERE book=OLD.id;
            DELETE FROM last_read_positions WHERE book=OLD.id;
            DELETE FROM annotations WHERE book=OLD.id;
            DELETE FROM comments WHERE book=OLD.id;
            DELETE FROM conversion_options WHERE book=OLD.id;
            DELETE FROM books_plugin_data WHERE book=OLD.id;
            DELETE FROM identifiers WHERE book=OLD.id;
        END;
        CREATE TRIGGER series_insert_trg AFTER INSERT ON series BEGIN
            UPDATE series SET sort=title_sort(NEW.name) WHERE id=NEW.id;
        END;
        CREATE TRIGGER fkc_insert_books_tags_link BEFORE INSERT ON books_tags_link BEGIN
            SELECT CASE
                WHEN (SELECT id from books WHERE id=NEW.book) IS NULL
                THEN RAISE(ABORT, 'Foreign key violation: book not in books')
                WHEN (SELECT id from tags WHERE id=NEW.tag) IS NULL
                THEN RAISE(ABORT, 'Foreign key violation: tag not in tags')
            END;
        END;
        CREATE TRIGGER fkc_delete_on_tags BEFORE DELETE ON tags BEGIN
            SELECT CASE
                WHEN (SELECT COUNT(id) FROM books_tags_link WHERE tag=OLD.id) > 0
                THEN RAISE(ABORT, 'Foreign key violation: tags is still referenced')
            END;
        END;
