CREATE TABLE posts (
    id INTEGER NOT NULL PRIMARY KEY,
    creator VARCHAR NOT NULL,
    title VARCHAR NOT NULL,
    tags TEXT NOT NULL,
    like_count INT NOT NULL,
    post_type VARCHAR NOT NULL
);

CREATE TABLE post_links (
    rowid INTEGER PRIMARY KEY NOT NULL,
    "url" VARCHAR NOT NULL UNIQUE,
    content_type VARCHAR NOT NULL,
    source VARCHAR NOT NULL,
    post_id INTEGER NOT NULL REFERENCES posts(id),
    "status" VARCHAR NOT NULL,
    error VARCHAR,
    file_path VARCHAR,
    file_path_pattern VARCHAR
);
