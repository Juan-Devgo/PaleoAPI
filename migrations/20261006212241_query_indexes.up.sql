-- Query-only indexes for the public read shapes (data-model §6.1, FR-011, FR-016).
-- Name sorts must repeat the exact expression: ORDER BY lower(<col>) COLLATE paleo_name_sort [DESC], id.

CREATE INDEX eras_start_mya_idx ON eras (start_mya, id);
CREATE INDEX periods_start_mya_idx ON periods (start_mya, id);
CREATE INDEX eras_name_sort_idx ON eras ((lower(name) COLLATE paleo_name_sort), id);
CREATE INDEX periods_name_sort_idx ON periods ((lower(name) COLLATE paleo_name_sort), id);
CREATE INDEX domains_name_sort_idx ON domains ((lower(name) COLLATE paleo_name_sort), id);
CREATE INDEX kingdoms_name_sort_idx ON kingdoms ((lower(name) COLLATE paleo_name_sort), id);
CREATE INDEX phyla_name_sort_idx ON phyla ((lower(name) COLLATE paleo_name_sort), id);
CREATE INDEX classes_name_sort_idx ON classes ((lower(name) COLLATE paleo_name_sort), id);
CREATE INDEX orders_name_sort_idx ON orders ((lower(name) COLLATE paleo_name_sort), id);
CREATE INDEX families_name_sort_idx ON families ((lower(name) COLLATE paleo_name_sort), id);
CREATE INDEX genera_name_sort_idx ON genera ((lower(name) COLLATE paleo_name_sort), id);
CREATE INDEX continents_name_sort_idx ON continents ((lower(name) COLLATE paleo_name_sort), id);
CREATE INDEX countries_name_sort_idx ON countries ((lower(name) COLLATE paleo_name_sort), id);
CREATE INDEX species_name_sort_idx ON species ((lower(name) COLLATE paleo_name_sort), id);
CREATE INDEX continents_type_idx ON continents (type, (lower(name) COLLATE paleo_name_sort), id);
CREATE INDEX species_scientific_name_sort_idx ON species ((lower(scientific_name) COLLATE paleo_name_sort), id);
CREATE INDEX species_diet_idx ON species (diet, (lower(name) COLLATE paleo_name_sort), id);
CREATE INDEX species_discovery_year_asc_idx ON species (discovery_year ASC NULLS LAST, id);
CREATE INDEX species_discovery_year_desc_idx ON species (discovery_year DESC NULLS LAST, id);

-- Substring search `q` (research R13).
CREATE INDEX species_name_trgm_idx ON species USING gin (name gin_trgm_ops);
CREATE INDEX species_scientific_name_trgm_idx ON species USING gin (scientific_name gin_trgm_ops);
