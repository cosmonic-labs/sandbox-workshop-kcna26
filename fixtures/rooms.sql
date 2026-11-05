-- A second database inside the sqlite tool's folder: the conference rooms.
-- Step 3 attaches it to show ATTACH works for a file the grant covers.
-- Invented data. Source of fixtures/rooms.db; after a change here, run
-- `make fixtures-db` and commit the .db and fixtures/SHA256SUMS.

create table rooms (
  name   text primary key,
  floor  integer not null,
  seats  integer not null
);

insert into rooms (name, floor, seats) values
  ('Hall A', 1, 900),
  ('Hall B', 1, 600),
  ('Room 1', 2, 120),
  ('Room 2', 2, 120);
