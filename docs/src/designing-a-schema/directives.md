# Directives

> Per GraphQL: A directive is a keyword preceded by a @ character (optionally followed by a list of named arguments) which can appear after almost any form of syntax in the GraphQL query or schema languages.

As of this writing, the list of supported Fuel GraphQL schema directives includes:

- `@indexed`: Denotes that a field should include a B-tree index in the database.
- `@unique`: Denotes that field should include a unique index in the database.
- `@join`: Denotes that a field has a "relationship" to another object type.

## `@indexed`

The `@indexed` directive adds a [database index](https://www.postgresql.org/docs/current/indexes-intro.html) to the underlying column for the indicated field of that type. Generally, a database index is a data structure that allows you to quickly locate data without having to search each row in a database table.

```graphql
type Book @entity {
    id: ID!
    name: Bytes8! @indexed
}

type Library @entity {
    id: ID!
    book: Book!
}
```

In this example, a single `BTREE INDEX` constraint will be created on the `book` table's `name` column, which allows for faster lookups on that field.

> Important: At the moment, database index constraint support is limited to `BTREE` in Postgres with `ON DELETE`, and `ON UPDATE` actions not being supported.

## `@unique`

The `@unique` directive adds a `UNIQUE` database constraint to the underlying database column for the indicated field of that type. A constraint specifies a rule for the data in a table and can be used to limit the type of data that can be placed in the table. In the case of a column with a `UNIQUE` constraint, all values in the column must be different.

```graphql
type Book @entity {
    id: ID!
    name: Bytes8! @unique
}

type Library @entity {
    id: ID!
    book: Book!
}
```

A `UNIQUE` constraint will be created on the `book` table's `name` column, ensuring that no books can share the same name.

> Important: When using explict or implicit foreign keys, it is required that the reference column name in your foreign key relationship be unique. `ID` types are by default unique, but all other types will have to be explicitly specified as being unique via the `@unique` directive.

## `@join`

The `@join` directive is used to relate a field in one type to others by referencing fields in another type. You can think of it as a link between two tables in your database. The field in the referenced type is called a _foreign key_ and it is **required** to be unique.

```graphql
type Book @entity {
    id: ID!
    name: Charfield! @unique
}

type Library @entity {
    id: ID!
    book: Book! @join(on:name)
}
```

A foreign key constraint will be created on `library.book` that references `book.name`, which relates the `Book`s in a `Library` to the underlying `Book` table. For more info on what exactly is happening here, please see the [Relationships](./relationships.md) section.
