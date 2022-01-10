# Reshape

[![Test status badge](https://github.com/fabianlindfors/Reshape/actions/workflows/test.yaml/badge.svg)](https://github.com/fabianlindfors/reshape/actions/workflows/test.yaml) [![Latest release](https://shields.io/github/v/release/fabianlindfors/reshape?display_name=tag&sort=semver&color=blue)](https://github.com/fabianlindfors/reshape/releases)

Reshape is an easy-to-use, zero-downtime schema migration tool for Postgres. It automatically handles complex migrations that would normally require downtime or manual multi-step changes. During a migration, Reshape ensures both the old and new schema are available at the same time, allowing you to gradually roll out your application. 

Reshape is designed for Postgres 12 and later.

*Note: Reshape is **experimental** and should not be used in production. It can (and probably will) destroy your data and break your application.*

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
	- [Columns](#columns)
		- [Add column](#add-column)
		- [Alter column](#alter-column)
		- [Remove column](#remove-column)
	- [Indices](#indices)
		- [Add index](#add-index)
- [Commands and options](#commands-and-options)
	- [`reshape migrate`](#reshape-migrate)
	- [`reshape complete`](#reshape-complete)
	- [`reshape abort`](#reshape-abort)
	- [`reshape generate-schema-query`](#reshape-generate-schema-query)
	- [Connection options](#connection-options)
- [How it works](#how-it-works)
- [License](#license)

## Getting started

### Installation

#### Binaries

Binaries are available for macOS and Linux under [Releases](https://github.com/fabianlindfors/reshape/releases).

#### Cargo

Reshape can be installed using [Cargo](https://doc.rust-lang.org/cargo/):

```shell
cargo install reshape
```

#### Docker

Reshape is available as a Docker image on [Docker Hub](https://hub.docker.com/repository/docker/fabianlindfors/reshape). 

```shell
docker run -v $(pwd):/usr/share/app fabianlindfors/reshape reshape migrate
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

Reshape relies on your application using a specific schema. When establishing the connection to Postgres in your application, you need to run a query to select the most recent schema. This query can be generated using: `reshape generate-schema-query`.

To pass it along to your application, you can for example use an environment variable in your run script: `RESHAPE_SCHEMA_QUERY=$(reshape generate-schema-query)`. Then in your application:

```python
# Example for Python
reshape_schema_query = os.getenv("RESHAPE_SCHEMA_QUERY")
db.execute(reshape_schema_query)
```

### Running your migration

To create your new `users` table, run:

```bash
reshape migrate
```

As this is the first migration, Reshape will automatically complete it. For subsequent migrations, you will need to first run `reshape migrate`, roll out your application and then complete the migration using `reshape complete`.

If nothing else is specified, Reshape will try to connect to a Postgres database running on `localhost` using `postgres` as both username and password. See [Connection options](#connection-options) for details on how to change the connection settings.

### Using during development

When adding new migrations during development, we recommend running `reshape migrate` but skipping `reshape complete`. This way, the new migrations can be iterated on by updating the migration file and running `reshape abort` followed by `reshape migrate`.

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

The `create_table` action will create a new table with the specified columns, indices and constraints.

*Example: create a `customers` table with a few columns and a primary key*

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

*Example: create `users` and `items` tables with a foreign key between them*

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

#### Rename table

The `rename_table` action will change the name of an existing table.

*Example: change name of `users` table to `customers`*

```toml
[[actions]]
type = "rename_table"
table = "users"
new_name = "customers"
```

#### Remove table

The `remove_table` action will remove an existing table.

*Example: remove `users` table*

```toml
[[actions]]
type = "remove_table"
table = "users"
```

### Columns

#### Add column

The `add_column` action will add a new column to an existing table. You can optionally provide an `up` setting. This should be an SQL expression which will be run for all existing rows to backfill the new column.

*Example: add a new column `reference` to table `products`*

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

*Example: replace an existing `name` column with two new columns, `first_name` and `last_name`*

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


#### Alter column

The `alter_column` action enables many different changes to an existing column, for example renaming, changing type and changing existing values.

When performing more complex changes than a rename, `up` and `down` must be provided. These should be SQL expressions which determine how to transform between the new and old version of the column. Inside those expressions, you can reference the current column value by the column name.

*Example: rename `last_name` column on `users` table to `family_name`*

```toml
[[actions]]
type = "alter_column"
table = "users"
column = "last_name"

	[actions.changes]
	name = "family_name"
```

*Example: change the type of `reference` column from `INTEGER` to `TEXT`*

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

*Example: increment all values of an `index` column by one*

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

*Example: change default value of `created_at` column to current time*

```toml
[[actions]]
type = "alter_column"
table = "users"
column = "created_at"

	[actions.changes]
	default = "NOW()"
```

#### Remove column

The `remove_column` action will remove an existing column from a table. You can optionally provide a `down` setting. This should be an SQL expression which will be used to determine values for the old schema when inserting or updating rows using the new schema. The `down` setting must be provided when the removed column is `NOT NULL` or doesn't have a default value.

*Example: remove column `name` from table `users`*

```toml
[[actions]]
type = "remove_column"
table = "users"
column = "name"

# Use a default value of "N/A" for the old schema when inserting/updating rows
down = "'N/A'"
```

### Indices

#### Add index

The `add_index` action will add a new index to an existing table.

*Example: create a `users` table with an index on the `name` column*

```toml
[[actions]]
type = "create_table"
table = "users"
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
name = "name_idx"
columns = ["name"]
```

## Commands and options

### `reshape migrate`

Starts a new migration, applying all migrations under `migrations/` that haven't yet been applied. After the command has completed, both the old and new schema will be usable at the same time. When you have rolled out the new version of your application which uses the new schema, you should run `reshape complete`.

#### Options

*See also [Connection options](#connection-options)*

| Option | Default | Description |
| ------ | ------- | ----------- |
| `--complete`, `-c` | `false` | Automatically complete migration after applying it. |
| `--dirs` | `migrations/` | Directories to search for migration files. Multiple directories can be specified using `--dirs dir1 dir2 dir3`. |

### `reshape complete`

Completes migrations previously started with `reshape complete`. 

#### Options

See [Connection options](#connection-options)

### `reshape abort`

Aborts any migrations which haven't yet been completed. 

#### Options

See [Connection options](#connection-options)

### `reshape generate-schema-query`

Generates the SQL query you need to run in your application before using the database. This command does not require a database connection. Instead it will generate the query based on the latest migration in the `migrations/` directory (or the directories specified by `--dirs`).

The query should look something like `SET search_path TO migration_1_initial_migration`.

#### Options

| Option | Default | Description |
| ------ | ------- | ----------- |
| `--dirs` | `migrations/` | Directories to search for migration files. Multiple directories can be specified using `--dirs dir1 dir2 dir3`. |

### Connection options

The options below can be used with all commands that communicate with Postgres. Use either a connection URL or specify each connection option individually.

| Option | Default | Description |
| ------ | ------- | ----------- |
| `--url`  | | URI to connect to your Postgres database.<br>Can also be provided with the environment variable `DATABASE_URL`. |
| `--host` | `localhost` | Hostname to use when connecting to Postgres |
| `--port` | `5432` | Port which Postgres is listening on |
| `--database` | `postgres` | Database name |
| `--username` | `postgres` | Postgres username |
| `--password` | `postgres` | Postgres password |

## How it works

Reshape works by creating views that encapsulate the underlying tables, which your application will interact with. During a migration, Reshape will automatically create a new set of views and set up triggers to translate inserts and updates between the old and new schema. This means that every deployment is a three-phase process:

1. **Start migration** (`reshape migrate`): Sets up views and triggers to ensure both the new and old schema are usable at the same time.
2. **Roll out application**: Your application can be gradually rolled out without downtime. The existing deployment will continue using the old schema whilst the new deployment uses the new schema.
3. **Complete migration** (`reshape complete`): Removes the old schema and any intermediate data and triggers. 

If the application deployment fails, you should run `reshape abort` which will roll back any changes made by `reshape migrate` without losing data.

## License

Reshape is released under the [MIT license](https://choosealicense.com/licenses/mit/).