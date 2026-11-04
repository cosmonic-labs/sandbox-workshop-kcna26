-- The database the sqlite tool reads in steps 1 and 4.
--
-- A small, invented conference schedule, so a query reads well on a slide and
-- no real person's data is involved. scripts/fixtures.sh builds main.db from
-- this file, so the database is never committed as a binary blob.

create table talks (
  id       integer primary key,
  title    text not null,
  speaker  text not null,
  track    text not null,
  room     text not null,
  day      text not null,
  start    text not null
);

insert into talks (title, speaker, track, room, day, start) values
  ('Sandboxing Agents and the Code They Write', 'A. Rivera',   'Security',    'Hall A',  'Wed', '09:00'),
  ('A Capability Model for WebAssembly',        'B. Chen',     'Wasm',        'Room 1',  'Wed', '10:30'),
  ('Least Privilege for Multi-Tenant Clusters', 'C. Okafor',   'Security',    'Hall B',  'Wed', '13:00'),
  ('Running Untrusted Code at Scale',           'D. Novak',    'Platform',    'Room 2',  'Wed', '14:30'),
  ('Components over Containers',                 'E. Larsson',  'Wasm',        'Room 1',  'Thu', '09:00'),
  ('Supply Chain Attacks in the Wild',          'F. Haddad',   'Security',    'Hall A',  'Thu', '10:30'),
  ('Scaling to a Million Workloads',            'G. Price',    'Platform',    'Hall B',  'Thu', '13:00'),
  ('Debugging Wasm in Production',              'H. Yamamoto', 'Wasm',        'Room 2',  'Thu', '14:30'),
  ('Secrets Without Tears',                     'I. Costa',    'Security',    'Room 1',  'Fri', '09:00'),
  ('From VMs to Sandboxes',                     'J. Dubois',   'Platform',    'Hall A',  'Fri', '10:30');
