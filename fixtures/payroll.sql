-- A database outside the sqlite tool's grant. scripts/up.sh puts it on the
-- host pod's own filesystem at /workshop/private-data/payroll.db, where native
-- sqlite3 on that machine would attach it. No workload mounts it, and the
-- host's hostPaths list doesn't allow it, so the component can't see it.
-- Invented data. Source of fixtures/payroll.db; after a change here, run
-- `make fixtures-db` and commit the .db and fixtures/SHA256SUMS.

create table salaries (
  name    text primary key,
  salary  integer not null
);

insert into salaries (name, salary) values
  ('A. Rivera', 182000),
  ('B. Chen',   174000),
  ('C. Okafor', 191000);
