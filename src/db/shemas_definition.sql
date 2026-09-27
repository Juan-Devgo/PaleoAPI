CREATE TABLE era(
	era_id VARCHAR(8) PRIMARY KEY,
	name VARCHAR(16) UNIQUE NOT NULL,
	start_mya INT NOT NULL,
	end_mya INT NOT NULL
);

CREATE TABLE period(
	period_id VARCHAR(8) PRIMARY KEY,
	name VARCHAR(16) UNIQUE NOT NULL,
	era VARCHAR(8) REFERENCES era(era_id) ON DELETE CASCADE,
	start_mya INT NOT NULL,
	end_mya INT NOT NULL
);

CREATE TABLE continent(
	continent_id VARCHAR(8) PRIMARY KEY,
	name VARCHAR(16) UNIQUE NOT NULL,
	continent_type VARCHAR(12) NOT NULL CHECK(continent_type IN ('PREHISTORIC', 'MODERN'))
);

CREATE TABLE country(
	country_id VARCHAR(8) PRIMARY KEY,
	name VARCHAR(16) UNIQUE NOT NULL
);

CREATE TABLE continent_country(
	continent VARCHAR(8) REFERENCES continent(continent_id) ON DELETE CASCADE,
	country VARCHAR(8) REFERENCES country(country_id) ON DELETE CASCADE,
	PRIMARY KEY(continent, country)
);

CREATE TABLE domain(
	domain_id VARCHAR(8) PRIMARY KEY,
	name VARCHAR(8) UNIQUE NOT NULL
);

CREATE TABLE kingdom(
	kingdom_id VARCHAR(8) PRIMARY KEY,
	name VARCHAR(8) UNIQUE NOT NULL
);

CREATE TABLE phylum(
	phylum_id VARCHAR(8) PRIMARY KEY,
	name VARCHAR(8) UNIQUE NOT NULL
);

CREATE TABLE class(
	class_id VARCHAR(8) PRIMARY KEY,
	name VARCHAR(8) UNIQUE NOT NULL
);

CREATE TABLE "order"(
	order_id VARCHAR(8) PRIMARY KEY,
	name VARCHAR(8) UNIQUE NOT NULL
);

CREATE TABLE family(
	family_id VARCHAR(8) PRIMARY KEY,
	name VARCHAR(8) UNIQUE NOT NULL
);

CREATE TABLE genus(
	genus_id VARCHAR(8) PRIMARY KEY,
	name VARCHAR(8) UNIQUE NOT NULL
);

CREATE TABLE taxonomy(
	taxonomy_id VARCHAR(8) PRIMARY KEY,
	domain VARCHAR(8) REFERENCES domain(domain_id) ON DELETE SET NULL,
	kingdom VARCHAR(8) REFERENCES kingdom(kingdom_id) ON DELETE SET NULL,
	phylum VARCHAR(8) REFERENCES phylum(phylum_id) ON DELETE SET NULL,
	class VARCHAR(8) REFERENCES class(class_id) ON DELETE SET NULL,
	"order" VARCHAR(8) REFERENCES "order"(order_id) ON DELETE SET NULL,
	family VARCHAR(8) REFERENCES family(family_id) ON DELETE SET NULL,
	genus VARCHAR(8) REFERENCES genus(genus_id) ON DELETE SET NULL
);

CREATE TABLE specie(
	specie_id VARCHAR(8) PRIMARY KEY,
	name VARCHAR(64) UNIQUE NOT NULL,
	scientific_name VARCHAR(64) UNIQUE NOT NULL,
	diet VARCHAR(16) NOT NULL CHECK(diet IN ('CARNIVORE', 'HERBIVORE', 'OMNIVORE', 'PISCIVORE', 'INSECTIVORE')),
	description VARCHAR(512) NOT NULL,
	taxonomy VARCHAR(8) REFERENCES taxonomy(taxonomy_id),
	discovery_year INT,
	image_url TEXT
);

CREATE TABLE specie_size(
	specie_size_id VARCHAR(8) PRIMARY KEY,
	min_length_m INT CHECK(min_length_m > 0),
	max_leght_m INT CHECK(min_length_m > 0),
	min_height_m INT CHECK(min_height_m > 0),
	max_height_m INT CHECK(max_height_m > 0),
	min_weight_kg INT CHECK(min_weight_kg > 0),
	max_weight_kg INT CHECK(max_weight_kg > 0),
	specie VARCHAR(8) REFERENCES specie(specie_id)
);

CREATE TABLE specie_period(
	specie VARCHAR(8) REFERENCES specie(specie_id),
	period VARCHAR(8) REFERENCES period(period_id),
	PRIMARY KEY(specie, period)
);

CREATE TABLE specie_continent(
	specie VARCHAR(8) REFERENCES specie(specie_id),
	continent VARCHAR(8) REFERENCES continent(continent_id),
	PRIMARY KEY(specie, continent)
);

CREATE TABLE role(
	role_id VARCHAR(8) PRIMARY KEY,
	name VARCHAR(12) UNIQUE NOT NULL
);

CREATE TABLE "user"(
	user_id UUID PRIMARY KEY,
	username VARCHAR(16) UNIQUE NOT NULL,
	email VARCHAR(320) UNIQUE NOT NULL,
	password_hash VARCHAR(256) NOT NULL,
	created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP 
);

CREATE TABLE user_role(
	"user" UUID REFERENCES "user"(user_id),
	role VARCHAR(8) REFERENCES role(role_id),
	PRIMARY KEY("user", role)
);


DROP TABLE era;
DROP TABLE peroid;
DROP TABLE country;
DROP TABLE continent;
DROP TABLE continent_country;