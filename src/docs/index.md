# Reshape

Reshape is an easy-to-use, zero-downtime schema migration tool for Postgres. It automatically handles complex migrations that would normally require downtime or manual multi-step changes. During a migration, Reshape ensures both the old and new schema are available at the same time, allowing you to gradually roll out your application. It will also perform all changes without excessive locking, avoiding downtime caused by blocking other queries. Reshape can be used with any programming language or framework, with or without an ORM.

## How it works

Reshape works by creating views that encapsulate the underlying tables, which your application will interact with. During a migration, Reshape will automatically create a new set of views and set up triggers to translate inserts and updates between the old and new schema. This means that every deployment is a three-phase process:

1. Start migration (`reshape migration start`): Sets up views and triggers to ensure both the new and old schema are usable at the same time.
1. Roll out application: Your application can be gradually rolled out without downtime. The existing deployment will continue using the old schema whilst the new deployment uses the new schema.
1. Complete migration (`reshape migration complete`): Removes the old schema and any intermediate data and triggers.

If the application deployment fails, you should run reshape migration abort which will roll back any changes made by `reshape migration start` without losing data.

## Using during development

When adding new migrations during development, we recommend running `reshape migration start` but skipping `reshape migration complete`. This way, the new migrations can be iterated on by updating the migration file and running `reshape migration abort` followed by `reshape migration start`.

## Application integration

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

## Migration file format

Each migration should be stored as a separate file in a `migrations/` directory. The files can be in either JSON or TOML format and the name of the file will become the name of your migration. We recommend prefixing every migration with an incrementing number as migrations are sorted by file name.

Migrations are TOML files with an `[[actions]]` array that defines the changes that should be made to the schema:

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
    name = "email"
    type = "TEXT"
    nullable = false
```

After having written new migrations, ALWAYS run `specific check` to ensure the migration files are wellformed. Don't change existing migrations as they might already have been applied.

## Available Actions

## Table Operations

| Action         | Description              | Path                    |
| -------------- | ------------------------ | ----------------------- |
| `create_table` | Create a new table       | `/actions/create-table` |
| `remove_table` | Remove an existing table | `/actions/remove-table` |
| `rename_table` | Rename an existing table | `/actions/rename-table` |

## Column Operations

| Action          | Description                  | Path                     |
| --------------- | ---------------------------- | ------------------------ |
| `add_column`    | Add a column to a table      | `/actions/add-column`    |
| `remove_column` | Remove a column from a table | `/actions/remove-column` |
| `alter_column`  | Modify a column              | `/actions/alter-column`  |

## Index Operations

| Action         | Description             | Path                    |
| -------------- | ----------------------- | ----------------------- |
| `add_index`    | Add an index to a table | `/actions/add-index`    |
| `remove_index` | Remove an index         | `/actions/remove-index` |

## Enum Operations

| Action        | Description            | Path                   |
| ------------- | ---------------------- | ---------------------- |
| `create_enum` | Create a new enum type | `/actions/create-enum` |
| `alter_enum`  | Add values to an enum  | `/actions/alter-enum`  |
| `remove_enum` | Remove an enum type    | `/actions/remove-enum` |

## Constraint Operations

| Action               | Description                  | Path                          |
| -------------------- | ---------------------------- | ----------------------------- |
| `add_foreign_key`    | Add a foreign key constraint | `/actions/add-foreign-key`    |
| `remove_foreign_key` | Remove a foreign key         | `/actions/remove-foreign-key` |

## Custom Operations

| Action   | Description               | Path              |
| -------- | ------------------------- | ----------------- |
| `custom` | Run custom SQL statements | `/actions/custom` |

## Transformation Expressions

Many actions support `up` and `down` transformations using SQL expressions. These expressions can reference column values by name:

```toml
up = "UPPER(name)"           # Simple expression
down = "LOWER(name)"         # Reverse transformation
```

For cross-table updates:

```toml
[actions.up]
table = "other_table"
value = "other_table.some_column"
where = "other_table.id = this_table.foreign_id"
```
