# Reshape

[![Test status badge](https://github.com/fabianlindfors/Reshape/actions/workflows/test.yaml/badge.svg)](https://github.com/fabianlindfors/reshape/actions/workflows/test.yaml) [![Latest release](https://shields.io/github/v/release/fabianlindfors/reshape?display_name=tag&sort=semver&color=blue)](https://github.com/fabianlindfors/reshape/releases)

> Also check out [ReshapeDB](https://reshapedb.com), a new database built from the ground up to make zero-downtime schema and data migrations as simple and safe as possible. If you'd like to chat about it, please [reach out](contact@reshapedb.com)!

Reshape is an easy-to-use, zero-downtime schema migration tool for Postgres. It automatically handles complex migrations that would normally require downtime or manual multi-step changes. During a migration, Reshape ensures both the old and new schema are available at the same time, allowing you to gradually roll out your application. It will also perform all changes without excessive locking, avoiding downtime caused by blocking other queries. For a more thorough introduction to Reshape, check out the [introductory blog post](https://fabianlindfors.se/blog/schema-migrations-in-postgres-using-reshape/).

Designed for Postgres 12 and later.

- [How it works](#how-it-works)
- [Getting started](#getting-started)
  - [Installation](#installation)
  - [Creating your first migration](#creating-your-first-migration)
  - [Preparing your application](#preparing-your-application)
  - [Running your migration](#running-your-migration)
  - [Using during development](#using-during-development)
- [Writing migrations](#writing-migrations)
  - [Basics](#basics)
  - [Tables](#tables)
    - [Create table](#create-table)
    - [Rename table](#rename-table)
    - [Remove table](#remove-table)
    - [Add foreign key](#add-foreign-key)
    - [Remove foreign key](#remove-foreign-key)
  - [Columns](#columns)
    - [Add column](#add-column)
    - [Alter column](#alter-column)
    - [Remove column](#remove-column)
  - [Indices](#indices)
    - [Add index](#add-index)
    - [Remove index](#remove-index)
  - [Enums](#enums)
    - [Create enum](#create-enum)
    - [Remove enum](#remove-enum)
  - [Custom](#custom)
  - [Complex changes across tables](#complex-changes-across-tables)
- [Commands and options](#commands-and-options)
  - [`reshape migration start`](#reshape-migration-start)
  - [`reshape migration complete`](#reshape-migration-complete)
  - [`reshape migration abort`](#reshape-migration-abort)
  - [`reshape schema-query`](#reshape-schema-query)
  - [Connection options](#connection-options)
- [License](#license)

## How it works

Reshape works by creating views that encapsulate the underlying tables, which your application will interact with. During a migration, Reshape will automatically create a new set of views and set up triggers to translate inserts and updates between the old and new schema. This means that every deployment is a three-phase process:

1. **Start migration** (`reshape migration start`): Sets up views and triggers to ensure both the new and old schema are usable at the same time.
2. **Roll out application**: Your application can be gradually rolled out without downtime. The existing deployment will continue using the old schema whilst the new deployment uses the new schema.
3. **Complete migration** (`reshape migration complete`): Removes the old schema and any intermediate data and triggers.

If the application deployment fails, you should run `reshape migration abort` which will roll back any changes made by `reshape migration start` without losing data.

## Getting started

### Installation

#### Binaries

Binaries are available for macOS and Linux under [Releases](https://github.com/fabianlindfors/reshape/releases).

#### Cargo

Reshape can be installed using [Cargo](https://doc.rust-lang.org/cargo/) (requires Rust 1.58 or later):

```shell
cargo install reshape
```

#### Docker

Reshape is available as a Docker image on [Docker Hub](https://hub.docker.com/repository/docker/fabianlindfors/reshape).

```shell
docker run -v $(pwd):/usr/share/app fabianlindfors/reshape reshape migration start
```

### Creating your first migration

Each migration should be stored as a separate file in a `migrations/` directory. The files can be in either JSON or TOML format and the name of the file will become the name of your migration. We recommend prefixing every migration with an incrementing number as migrations are sorted by file name.

Let's create a simple migration to set up a new table `users` with two fields, `id` and `name`. We'll create a file called `migrations/1_create_users_table.toml`:

```toml
[[actions]]
type = "create_table"
name = "users"
primary_key = ["id"]

	[[actions.columns]]
	name = "id"
	type = "INTEGER"
	generated = "ALWAYS AS IDENTITY"

	[[actions.columns]]
	name = "name"
	type = "TEXT"
```

This is the equivalent of running `CREATE TABLE users (id INTEGER GENERATED ALWAYS AS IDENTITY, name TEXT)`.

### Preparing your application

Reshape relies on your application using a specific schema. When establishing the connection to Postgres in your application, you need to run a query to select the most recent schema. The simplest way to do this is to use one of the helper libraries:

- [Rust](https://github.com/fabianlindfors/reshape-helper)
- [Ruby (and Rails)](https://github.com/fabianlindfors/reshape-ruby)
- [Python (and Django)](https://github.com/fabianlindfors/reshape-python)
- [Go](https://github.com/leourbina/reshape-helper)

If your application is not using one of the languages with an available helper library, you can instead generate the query with the command: `reshape schema-query`. To pass it along to your application, you can for example use an environment variable in your run script: `RESHAPE_SCHEMA_QUERY=$(reshape schema-query)`. Then in your application:

```python
# Example for Python
reshape_schema_query = os.getenv("RESHAPE_SCHEMA_QUERY")
db.execute(reshape_schema_query)
```

### Running your migration

To create your new `users` table, run:

```bash
reshape migration start --complete
```

We use the `--complete` flag to automatically complete the migration. During a production deployment, you should first run `reshape migration start` followed by `reshape migration complete` once your application has been fully rolled out.

If nothing else is specified, Reshape will try to connect to a Postgres database running on `localhost` using `postgres` as both username and password. See [Connection options](#connection-options) for details on how to change the connection settings.

### Using during development

When adding new migrations during development, we recommend running `reshape migration start` but skipping `reshape migration complete`. This way, the new migrations can be iterated on by updating the migration file and running `reshape migration abort` followed by `reshape migration start`.

## Writing migrations

### Basics

Every migration consists of one or more actions. The actions will be run sequentially. Here's an example of a migration with two actions to create two tables, `customers` and `products`:

```toml
[[actions]]
type = "create_table"
name = "customers"
primary_key = ["id"]

	[[actions.columns]]
	name = "id"
	type = "INTEGER"
	generated = "ALWAYS AS IDENTITY"

[[actions]]
type = "create_table"
name = "products"
primary_key = ["sku"]

	[[actions.columns]]
	name = "sku"
	type = "TEXT"
```

Every action has a `type`. The supported types are detailed below.

### Tables

#### Create table

The `create_table` action will create a new table with the specified columns, indices and constraints. You can optionally provide an `up` option to backfill values from an existing table.

_Example: create a `customers` table with a few columns and a primary key_

```toml
[[actions]]
type = "create_table"
name = "customers"
primary_key = ["id"]

	[[actions.columns]]
	name = "id"
	type = "INTEGER"
	generated = "ALWAYS AS IDENTITY"

	[[actions.columns]]
	name = "name"
	type = "TEXT"

	# Columns default to nullable
	nullable = false

	# default can be any valid SQL value, in this case a string literal
	default = "'PLACEHOLDER'"
```

_Example: create `users` and `items` tables with a foreign key between them_

```toml
[[actions]]
type = "create_table"
name = "users"
primary_key = ["id"]

	[[actions.columns]]
	name = "id"
	type = "INTEGER"
	generated = "ALWAYS AS IDENTITY"

[[actions]]
type = "create_table"
name = "items"
primary_key = ["id"]

	[[actions.columns]]
	name = "id"
	type = "INTEGER"
	generated = "ALWAYS AS IDENTITY"

	[[actions.columns]]
	name = "user_id"
	type = "INTEGER"

	[[actions.foreign_keys]]
	columns = ["user_id"]
	referenced_table = "users"
	referenced_columns = ["id"]
```

_Example: create `profiles` table based on existing `users` table_

```toml
[[actions]]
type = "create_table"
name = "profiles"
primary_key = ["user_id"]

	[[actions.columns]]
	name = "user_id"
	type = "INTEGER"

	[[actions.columns]]
	name = "user_email"
	type = "TEXT"

	# Backfill from `users` table and copy `users.email` to `user_email` column
	# This will perform an upsert based on the primary key to avoid duplicate rows
	[actions.up]
	table = "users"
	values = { user_id = "id", user_email = "email" }
```

#### Rename table

The `rename_table` action will change the name of an existing table.

_Example: change name of `users` table to `customers`_

```toml
[[actions]]
type = "rename_table"
table = "users"
new_name = "customers"
```

#### Remove table

The `remove_table` action will remove an existing table.

_Example: remove `users` table_

```toml
[[actions]]
type = "remove_table"
table = "users"
```

#### Add foreign key

The `add_foreign_key` action will add a foreign key between two existing tables. The migration will fail if the existing column values aren't valid references.

_Example: create foreign key from `items` to `users` table_

```toml
[[actions]]
type = "add_foreign_key"
table = "items"

	[actions.foreign_key]
	columns = ["user_id"]
	referenced_table = "users"
	referenced_columns = ["id"]
```

#### Remove foreign key

The `remove_foreign_key` action will remove an existing foreign key. The foreign key will only be removed once the migration is completed, which means that your new application must continue to adhere to the foreign key constraint.

_Example: remove foreign key `items_user_id_fkey` from `users` table_

```toml
[[actions]]
type = "remove_foreign_key"
table = "items"
foreign_key = "items_user_id_fkey"
```

### Columns

#### Add column

The `add_column` action will add a new column to an existing table. You can optionally provide an `up` setting. This should be an SQL expression which will be run for all existing rows to backfill the new column. `up` may also reference another table to perform cross-table migrations (see ["Complex changes across tables"](#complex-changes-across-tables)).

_Example: add a new column `reference` to table `products`_

```toml
[[actions]]
type = "add_column"
table = "products"

	[actions.column]
	name = "reference"
	type = "INTEGER"
	nullable = false
	default = "10"
```

_Example: replace an existing `name` column with two new columns, `first_name` and `last_name`_

```toml
[[actions]]
type = "add_column"
table = "users"

# Extract the first name from the existing name column
up = "(STRING_TO_ARRAY(name, ' '))[1]"

	[actions.column]
	name = "first_name"
	type = "TEXT"


[[actions]]
type = "add_column"
table = "users"

# Extract the last name from the existing name column
up = "(STRING_TO_ARRAY(name, ' '))[2]"

	[actions.column]
	name = "last_name"
	type = "TEXT"


[[actions]]
type = "remove_column"
table = "users"
column = "name"

# Reconstruct name column by concatenating first and last name
down = "first_name || ' ' || last_name"
```

_Example: extract nested value from unstructured JSON `data` column to new `name` column_

```toml
[[actions]]
type = "add_column"
table = "users"

# #>> '{}' converts the JSON string value to TEXT
up = "data['path']['to']['value'] #>> '{}'"

	[actions.column]
	name = "name"
	type = "TEXT"
```

_Example: duplicate `email` column from `users` to `profiles` table_

```toml
# `profiles` has `user_id` column which maps to `users.id`
[[actions]]
type = "add_column"
table = "profiles"

	[actions.column]
	name = "email"
	type = "TEXT"
	nullable = false

	# When `users` is updated in the old schema, we write the email value to `profiles`
	[actions.up]
	table = "users"
	value = "email"
	where = "user_id = id"
```

#### Alter column

The `alter_column` action enables many different changes to an existing column, for example renaming, changing type and changing existing values.

When performing more complex changes than a rename, `up` and `down` should be provided. These should be SQL expressions which determine how to transform between the new and old version of the column. Inside those expressions, you can reference the current column value by the column name.

_Example: rename `last_name` column on `users` table to `family_name`_

```toml
[[actions]]
type = "alter_column"
table = "users"
column = "last_name"

	[actions.changes]
	name = "family_name"
```

_Example: change the type of `reference` column from `INTEGER` to `TEXT`_

```toml
[[actions]]
type = "alter_column"
table = "users"
column = "reference"

up = "CAST(reference AS TEXT)" # Converts from integer value to text
down = "CAST(reference AS INTEGER)" # Converts from text value to integer

	[actions.changes]
	type = "TEXT" # Previous type was 'INTEGER'
```

_Example: increment all values of an `index` column by one_

```toml
[[actions]]
type = "alter_column"
table = "users"
column = "index"

up = "index + 1" # Increment for new schema
down = "index - 1" # Decrement to revert for old schema

	[actions.changes]
	name = "index"
```

_Example: make `name` column not nullable_

```toml
[[actions]]
type = "alter_column"
table = "users"
column = "name"

# Use "N/A" for any rows that currently have a NULL name
up = "COALESCE(name, 'N/A')"

	[actions.changes]
	nullable = false
```

_Example: change default value of `created_at` column to current time_

```toml
[[actions]]
type = "alter_column"
table = "users"
column = "created_at"

	[actions.changes]
	default = "NOW()"
```

#### Remove column

The `remove_column` action will remove an existing column from a table. You can optionally provide a `down` setting. This should be an SQL expression which will be used to determine values for the old schema when inserting or updating rows using the new schema. `down` may also reference another table to perform cross-table migrations (see ["Complex changes across tables"](#complex-changes-across-tables)) . The `down` setting must be provided when the removed column is `NOT NULL` or doesn't have a default value.

Any indices that cover the column will be removed.

_Example: remove column `name` from table `users`_

```toml
[[actions]]
type = "remove_column"
table = "users"
column = "name"

# Use a default value of "N/A" for the old schema when inserting/updating rows
down = "'N/A'"
```

_Example: remove `email` column from `users` table and use column from `profiles` table instead_

```toml
[[actions]]
type = "remove_column"
table = "users"
column = "email"

	# Our application will use the `profiles.email` column instead
	# For backwards compatibility, we will write back to the removed `email` column whenever `profiles` is changed
	[actions.down]
	table = "profiles"
	value = "profiles.email"
	where = "users.id = profiles.user_id"
```

### Indices

#### Add index

The `add_index` action will add a new index to an existing table.

_Example: create a `users` table with a unique index on the `name` column_

```toml
[[actions]]
type = "create_table"
name = "users"
primary_key = "id"

	[[actions.columns]]
	name = "id"
	type = "INTEGER"
	generated = "ALWAYS AS IDENTITY"

	[[actions.columns]]
	name = "name"
	type = "TEXT"

[[actions]]
type = "add_index"
table = "users"

	[actions.index]
	name = "name_idx"
	columns = ["name"]

	# Defaults to false
	unique = true
```

_Example: add GIN index to `data` column on `products` table_

```toml
[[actions]]
type = "add_index"
table = "products"

	[actions.index]
	name = "data_idx"
	columns = ["data"]

	# One of: btree (default), hash, gist, spgist, gin, brin
	type = "gin"
```

#### Remove index

The `remove_index` action will remove an existing index. The index won't actually be removed until the migration is completed.

_Example: remove the `name_idx` index_

```toml
[[actions]]
type = "remove_index"
index = "name_idx"
```

### Enums

#### Create enum

The `create_enum` action will create a new [enum type](https://www.postgresql.org/docs/current/datatype-enum.html) with the specified values.

_Example: add a new `mood` enum type with three possible values_

```toml
[[actions]]
type = "create_enum"
name = "mood"
values = ["happy", "ok", "sad"]
```

#### Remove enum

The `remove_enum` action will remove an existing [enum type](https://www.postgresql.org/docs/current/datatype-enum.html). Make sure all usages of the enum has been removed before running the migration. The enum will only be removed once the migration is completed.

_Example: remove the `mood` enum type_

```toml
[[actions]]
type = "remove_enum"
enum = "mood"
```

### Custom

The `custom` action lets you create a migration which runs custom SQL. It should be used with great care as it provides no guarantees of zero-downtime and will simply run whatever SQL is provided. Use other actions whenever possible as they are explicitly designed for zero downtime.

There are three optional settings available which all accept SQL queries. All queries need to be idempotent, for example by using `IF NOT EXISTS` wherever available.

- `start`: run when a migration is started using `reshape migration start`
- `complete`: run when a migration is completed using `reshape migration complete`
- `abort`: run when a migration is aborted using `reshape migration abort`

_Example: enable PostGIS and pg_stat_statements extensions_

```toml
[[actions]]
type = "custom"

start = """
	CREATE EXTENSION IF NOT EXISTS postgis;
	CREATE EXTENSION IF NOT EXISTS pg_stat_statements;
"""

abort = """
	DROP EXTENSION IF EXISTS postgis;
	DROP EXTENSION IF EXISTS pg_stat_statements;
"""
```

### Complex changes across tables

The `up` and `down` options available when creating tables, adding columns and removing columns can also perform more complex changes that span tables.

_Example: move `email` column from `users` to `profiles` table_

```toml
[[actions]]
type = "add_column"
table = "profiles"

	[actions.column]
	name = "email"
	type = "TEXT"
	nullable = false

	# When `users` is updated in the old schema, we write the email value to `profiles`
	[actions.up]
	table = "users"
	value = "users.email"
	where = "profiles.user_id = users.id"

[[actions]]
type = "remove_column"
table = "users"
column = "email"

	# When `profiles` is changed in the new schema, we write the email address back to the removed column
	[actions.down]
	table = "profiles"
	value = "profiles.email"
	where = "users.id = profiles.user_id"
```

_Example: turn a 1:N relationship between `users` and `accounts` into N:M and change the format of the associated `role`_

```toml
# Add `user_account_connections` as a junction table
[[actions]]
type = "create_table"
name = "user_account_connections"
primary_key = ["account_id", "user_id"]

	[[actions.columns]]
	name = "account_id"
	type = "INTEGER"

	[[actions.columns]]
	name = "user_id"
	type = "INTEGER"

	# `role` is currently stored directly on the `users` table but is part of the relationship
	[[actions.columns]]
	name = "role"
	type = "TEXT"

	# Backfill the new table from `users` and uppercase the `role`
	[actions.up]
	table = "users"
	values = { user_id = "id", account_id = "account_id", role = "UPPER(account_role)" }
	where = "user_account_connections.user_id = users.id"

[[actions]]
type = "remove_column"
table = "users"
column = "account_id"

	# When `user_account_connections` is updated, we write the `account_id` back to `users`
	[actions.down]
	table = "user_account_connections"
	value = "user_account_connections.account_id"
	where = "users.id = user_account_connections.user_id"

[[actions]]
type = "remove_column"
table = "users"
column = "account_role"

	# When `user_account_connections` is updated, we write the lowercase role back to `users`
	[actions.down]
	table = "user_account_connections"
	value = "LOWER(user_account_connections.role)"
	where = "users.id = user_account_connections.user_id"
```

## Commands and options

### `reshape migration start`

Starts a new migration, applying all migrations under `migrations/` that haven't yet been applied. After the command has completed, both the old and new schema will be usable at the same time. When you have rolled out the new version of your application which uses the new schema, you should run `reshape migration complete`.

#### Options

_See also [Connection options](#connection-options)_

| Option             | Default       | Description                                                                                                     |
| ------------------ | ------------- | --------------------------------------------------------------------------------------------------------------- |
| `--complete`, `-c` | `false`       | Automatically complete migration after applying it.                                                             |
| `--dirs`           | `migrations/` | Directories to search for migration files. Multiple directories can be specified using `--dirs dir1 dir2 dir3`. |

### `reshape migration complete`

Completes migrations previously started with `reshape migration complete`.

#### Options

See [Connection options](#connection-options)

### `reshape migration abort`

Aborts any migrations which haven't yet been completed.

#### Options

See [Connection options](#connection-options)

### `reshape schema-query`

Generates the SQL query you need to run in your application before using the database. This command does not require a database connection. Instead it will generate the query based on the latest migration in the `migrations/` directory (or the directories specified by `--dirs`).

If your application is written in Rust, we recommend using the [Rust helper library](https://github.com/fabianlindfors/reshape-helper/) instead.

The query should look something like `SET search_path TO migration_1_initial_migration`.

#### Options

| Option   | Default       | Description                                                                                                     |
| -------- | ------------- | --------------------------------------------------------------------------------------------------------------- |
| `--dirs` | `migrations/` | Directories to search for migration files. Multiple directories can be specified using `--dirs dir1 dir2 dir3`. |

### Connection options

The options below can be used with all commands that communicate with Postgres. Use either a [connection URL](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNSTRING) or specify each connection option individually.

All options can also be set using environment variables instead of flags. If a `.env` file exists, then variables will be automatically loaded from there.

| Option       | Default     | Environment variable | Description                                 |
| ------------ | ----------- | -------------------- | ------------------------------------------- |
| `--url`      |             | `DB_URL`             | URL to your Postgres database               |
| `--host`     | `localhost` | `DB_HOST`            | Hostname to use when connecting to Postgres |
| `--port`     | `5432`      | `DB_PORT`            | Port which Postgres is listening on         |
| `--database` | `postgres`  | `DB_NAME`            | Database name                               |
| `--username` | `postgres`  | `DB_USERNAME`        | Postgres username                           |
| `--password` | `postgres`  | `DB_PASSWORD`        | Postgres password                           |

## License

Reshape is released under the [MIT license](https://choosealicense.com/licenses/mit/).
