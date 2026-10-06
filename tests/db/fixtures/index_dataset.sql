-- Index-verification dataset (data-model §9). Test-only, not seed data.
-- Deterministic (generate_series, no random()); every row satisfies every rule.
--
-- Names are pronounceable syllable codes (16 two-character syllables, some
-- accented). No syllable contains "r" or "sa", so the documented `q` terms
-- match only the rows that inject them:
--   * 3-character term "rax":    species.name ends in "rax" when n % 40 = 0         (250 rows, 2.5%)
--   * longer term     "saurus": species.scientific_name genus part ends in "saurus"
--                               when n % 37 = 5                                    (270 rows, 2.7%)

BEGIN;

CREATE TEMPORARY TABLE syl (i int PRIMARY KEY, s text NOT NULL) ON COMMIT DROP;
INSERT INTO syl
SELECT ord - 1, s
  FROM unnest(ARRAY['ba','ce','di','fo','gu','la','me','ni','po','tu','sé','vi','ko','zu','ñe','mo'])
       WITH ORDINALITY AS t(s, ord);

-- enc(n): four syllables, unique for 0 <= n < 65536.
CREATE TEMPORARY TABLE codes (cn int PRIMARY KEY, code text NOT NULL) ON COMMIT DROP;
INSERT INTO codes
SELECT n, a.s || b.s || c.s || d.s
  FROM generate_series(0, 65535) AS n
  JOIN syl a ON a.i = n % 16
  JOIN syl b ON b.i = (n / 16) % 16
  JOIN syl c ON c.i = (n / 256) % 16
  JOIN syl d ON d.i = (n / 4096) % 16;

-- 12 contiguous eras inside 0–3600, 5 contiguous periods each.
INSERT INTO eras (id, name, start_mya, end_mya)
SELECT format('era-%s', lpad(i::text, 2, '0')), 'Era ' || initcap(code), i * 300, (i - 1) * 300
  FROM generate_series(1, 12) AS i JOIN codes ON codes.cn = i;

INSERT INTO periods (id, era_id, name, start_mya, end_mya)
SELECT format('period-%s', lpad(p::text, 2, '0')),
       format('era-%s', lpad((((p - 1) / 5) + 1)::text, 2, '0')),
       'Period ' || initcap(code),
       ((p - 1) / 5) * 300 + (((p - 1) % 5) + 1) * 60,
       ((p - 1) / 5) * 300 + ((p - 1) % 5) * 60
  FROM generate_series(1, 60) AS p JOIN codes ON codes.cn = p;

-- Taxonomy: 3 / 12 / 60 / 240 / 800 / 2000 / 5000, parents round-robin.
INSERT INTO domains (id, name)
SELECT format('domain-%s', i), initcap(code) FROM generate_series(1, 3) AS i JOIN codes ON codes.cn = i;
INSERT INTO kingdoms (id, name, domain_id)
SELECT format('kingdom-%s', i), initcap(code), format('domain-%s', (i - 1) % 3 + 1)
  FROM generate_series(1, 12) AS i JOIN codes ON codes.cn = i;
INSERT INTO phyla (id, name, kingdom_id)
SELECT format('phylum-%s', i), initcap(code), format('kingdom-%s', (i - 1) % 12 + 1)
  FROM generate_series(1, 60) AS i JOIN codes ON codes.cn = i;
INSERT INTO classes (id, name, phylum_id)
SELECT format('class-%s', i), initcap(code), format('phylum-%s', (i - 1) % 60 + 1)
  FROM generate_series(1, 240) AS i JOIN codes ON codes.cn = i;
INSERT INTO orders (id, name, class_id)
SELECT format('order-%s', i), initcap(code), format('class-%s', (i - 1) % 240 + 1)
  FROM generate_series(1, 800) AS i JOIN codes ON codes.cn = i;
INSERT INTO families (id, name, order_id)
SELECT format('family-%s', i), initcap(code), format('order-%s', (i - 1) % 800 + 1)
  FROM generate_series(1, 2000) AS i JOIN codes ON codes.cn = i;
INSERT INTO genera (id, name, family_id)
SELECT format('genus-%s', i), initcap(code), format('family-%s', (i - 1) % 2000 + 1)
  FROM generate_series(1, 5000) AS i JOIN codes ON codes.cn = i;

-- 16 continents (1–8 modern, 9–16 prehistoric); 200 countries with 1–2 modern continents.
INSERT INTO continents (id, name, type)
SELECT format('continent-%s', i), initcap(code), CASE WHEN i <= 8 THEN 'modern' ELSE 'prehistoric' END
  FROM generate_series(1, 16) AS i JOIN codes ON codes.cn = i;

INSERT INTO countries (id, name)
SELECT format('country-%s', i), initcap(code) FROM generate_series(1, 200) AS i JOIN codes ON codes.cn = i;
INSERT INTO country_continents (country_id, continent_id)
SELECT format('country-%s', i), format('continent-%s', (i - 1) % 8 + 1) FROM generate_series(1, 200) AS i
UNION ALL
SELECT format('country-%s', i), format('continent-%s', i % 8 + 1) FROM generate_series(2, 200, 2) AS i;

-- 10,000 species: every diet, ~1/7 without discovery_year, half with a size.
INSERT INTO species (id, genus_id, name, scientific_name, diet, description, discovery_year,
                     image_url, length_min_m, length_max_m, height_min_m, height_max_m,
                     weight_min_kg, weight_max_kg)
SELECT format('species-%s', n),
       format('genus-%s', (n - 1) % 5000 + 1),
       initcap(c1.code) || CASE WHEN n % 40 = 0 THEN 'rax' ELSE '' END,
       initcap(c1.code) || CASE WHEN n % 37 = 5 THEN 'saurus' ELSE '' END || ' ' || c2.code,
       (ARRAY['carnivore', 'herbivore', 'omnivore', 'piscivore', 'insectivore'])[n % 5 + 1],
       'Synthetic species number ' || n || '.',
       CASE WHEN n % 7 = 0 THEN NULL ELSE 1800 + n % 220 END,
       CASE WHEN n % 3 = 0 THEN format('https://example.org/species/%s.png', n) END,
       CASE WHEN n % 2 = 0 THEN (n % 100) / 10.0 + 0.5 END,
       CASE WHEN n % 2 = 0 THEN (n % 100) / 10.0 + 1.75 END,
       CASE WHEN n % 4 = 0 THEN (n % 50) / 10.0 + 0.25 END,
       CASE WHEN n % 4 = 0 THEN (n % 50) / 10.0 + 0.5 END,
       CASE WHEN n % 6 = 0 THEN n % 9000 + 0.125 END,
       CASE WHEN n % 6 = 0 THEN n % 9000 + 10.5 END
  FROM generate_series(1, 10000) AS n
  JOIN codes c1 ON c1.cn = n
  JOIN codes c2 ON c2.cn = (n * 7) % 65536;

-- 1–3 periods per species (≈ 20,000 links).
INSERT INTO species_periods (species_id, period_id)
SELECT format('species-%s', n), format('period-%s', lpad(((n - 1) % 60 + 1)::text, 2, '0'))
  FROM generate_series(1, 10000) AS n
UNION ALL
SELECT format('species-%s', n), format('period-%s', lpad((n % 60 + 1)::text, 2, '0'))
  FROM generate_series(1, 10000) AS n WHERE n % 3 <> 0
UNION ALL
SELECT format('species-%s', n), format('period-%s', lpad(((n + 7) % 60 + 1)::text, 2, '0'))
  FROM generate_series(1, 10000) AS n WHERE n % 3 = 2;

-- 0–3 continents per species (≈ 15,000 links).
INSERT INTO species_continents (species_id, continent_id)
SELECT format('species-%s', n), format('continent-%s', (n + k) % 16 + 1)
  FROM generate_series(1, 10000) AS n, generate_series(0, 2) AS k
 WHERE k < n % 4;

-- 0–4 countries per species (≈ 20,000 links).
INSERT INTO species_countries (species_id, country_id)
SELECT format('species-%s', n), format('country-%s', (n + k * 41) % 200 + 1)
  FROM generate_series(1, 10000) AS n, generate_series(0, 3) AS k
 WHERE k < n % 5;

COMMIT;
