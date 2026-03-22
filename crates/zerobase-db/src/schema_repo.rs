//! Concrete [`SchemaRepository`] implementation backed by SQLite.
//!
//! Manages the `_collections` and `_fields` metadata tables, and performs DDL
//! (CREATE TABLE, ALTER TABLE, DROP TABLE) on user-defined collection tables.

use rusqlite::{params, Connection, Transaction};
use serde_json;
use tracing::{debug, info};
use zerobase_core::generate_id;

use crate::error::{DbError, Result};
use crate::fts;
use crate::{
    CollectionSchema, ColumnDef, Database, IndexColumnDef, IndexColumnSort, IndexDef,
    SchemaAlteration, SchemaRepository,
};

// ── Implementation ────────────────────────────────────────────────────────────

impl SchemaRepository for Database {
    fn list_collections(&self) -> Result<Vec<CollectionSchema>> {
        let conn = self.read_conn()?;
        list_collections_impl(&conn)
    }

    fn get_collection(&self, name: &str) -> Result<CollectionSchema> {
        let conn = self.read_conn()?;
        get_collection_impl(&conn, name)
    }

    fn create_collection(&self, schema: &CollectionSchema) -> Result<()> {
        self.transaction(|tx| create_collection_impl(tx, schema))
    }

    fn update_collection(&self, name: &str, schema: &CollectionSchema) -> Result<()> {
        self.transaction(|tx| update_collection_impl(tx, name, schema))
    }

    fn alter_collection(&self, name: &str, alteration: &SchemaAlteration) -> Result<()> {
        self.transaction(|tx| alter_collection_impl(tx, name, alteration))
    }

    fn delete_collection(&self, name: &str) -> Result<()> {
        self.transaction(|tx| delete_collection_impl(tx, name))
    }

    fn collection_exists(&self, name: &str) -> Result<bool> {
        let conn = self.read_conn()?;
        collection_exists_impl(&conn, name)
    }
}

// ── Query helpers ─────────────────────────────────────────────────────────────

fn list_collections_impl(conn: &Connection) -> Result<Vec<CollectionSchema>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, name, type, view_query FROM _collections WHERE system = 0 ORDER BY name",
        )
        .map_err(DbError::Query)?;

    let rows: Vec<(String, String, String, Option<String>)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(DbError::Query)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(DbError::Query)?;

    let mut collections = Vec::with_capacity(rows.len());
    for (coll_id, name, coll_type, view_query) in rows {
        let columns = load_columns(conn, &coll_id)?;
        let indexes = load_indexes(conn, &name)?;
        collections.push(CollectionSchema {
            name,
            collection_type: coll_type,
            columns,
            indexes,
            searchable_fields: vec![],
            view_query,
        });
    }

    Ok(collections)
}

fn get_collection_impl(conn: &Connection, name: &str) -> Result<CollectionSchema> {
    let (coll_id, coll_type, view_query): (String, String, Option<String>) = conn
        .query_row(
            "SELECT id, type, view_query FROM _collections WHERE name = ?1",
            params![name],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => DbError::not_found_with_id("Collection", name),
            other => DbError::Query(other),
        })?;

    let columns = if coll_type == "view" {
        infer_view_columns(conn, name)?
    } else {
        load_columns(conn, &coll_id)?
    };
    let indexes = load_indexes(conn, name)?;

    Ok(CollectionSchema {
        name: name.to_string(),
        collection_type: coll_type,
        columns,
        indexes,
        searchable_fields: vec![],
        view_query,
    })
}

fn create_collection_impl(tx: &Transaction<'_>, schema: &CollectionSchema) -> Result<()> {
    // Check for duplicate name.
    let exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM _collections WHERE name = ?1)",
            params![schema.name],
            |row| row.get(0),
        )
        .map_err(DbError::Query)?;

    if exists {
        return Err(DbError::conflict(format!(
            "collection '{}' already exists",
            schema.name
        )));
    }

    // Generate a unique collection ID.
    let coll_id = generate_id();

    // Insert into _collections (including view_query for view collections).
    tx.execute(
        "INSERT INTO _collections (id, name, type, view_query) VALUES (?1, ?2, ?3, ?4)",
        params![coll_id, schema.name, schema.collection_type, schema.view_query],
    )
    .map_err(DbError::Query)?;

    if schema.collection_type == "view" {
        // View collections: create a SQL VIEW.
        let view_query = schema.view_query.as_deref().ok_or_else(|| {
            DbError::schema("view collections must have a view_query".to_string())
        })?;
        create_view(tx, &schema.name, view_query)?;
    } else {
        // Base/auth collections: create a table + indexes.
        insert_fields(tx, &coll_id, &schema.columns)?;
        create_user_table(tx, schema)?;

        // Create FTS5 index if there are searchable fields.
        if !schema.searchable_fields.is_empty() {
            let field_refs: Vec<&str> = schema
                .searchable_fields
                .iter()
                .map(|s| s.as_str())
                .collect();
            fts::create_fts_index(tx, &schema.name, &field_refs)?;
        }
    }

    info!(collection = %schema.name, "Created collection");
    Ok(())
}

fn update_collection_impl(
    tx: &Transaction<'_>,
    name: &str,
    schema: &CollectionSchema,
) -> Result<()> {
    // Look up the existing collection.
    let (coll_id, _old_type): (String, String) = tx
        .query_row(
            "SELECT id, type FROM _collections WHERE name = ?1",
            params![name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => DbError::not_found_with_id("Collection", name),
            other => DbError::Query(other),
        })?;

    // If the name changed, check the new name isn't taken.
    if schema.name != name {
        let name_taken: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM _collections WHERE name = ?1)",
                params![schema.name],
                |row| row.get(0),
            )
            .map_err(DbError::Query)?;

        if name_taken {
            return Err(DbError::conflict(format!(
                "collection '{}' already exists",
                schema.name
            )));
        }
    }

    // Update _collections metadata.
    tx.execute(
        "UPDATE _collections SET name = ?1, type = ?2, updated = datetime('now') WHERE id = ?3",
        params![schema.name, schema.collection_type, coll_id],
    )
    .map_err(DbError::Query)?;

    // Load old columns for diffing.
    let old_columns = load_columns_from_tx(tx, &coll_id)?;

    // Replace all fields: delete old, insert new.
    tx.execute(
        "DELETE FROM _fields WHERE collection_id = ?1",
        params![coll_id],
    )
    .map_err(DbError::Query)?;
    insert_fields(tx, &coll_id, &schema.columns)?;

    // Alter the user table using SQLite's rebuild strategy.
    alter_user_table(tx, name, schema, &old_columns)?;

    // Recreate FTS index: drop the old one, create a new one if there are searchable fields.
    let effective_name = &schema.name;
    if fts::fts_table_exists(tx, name).unwrap_or(false) {
        fts::drop_fts_index(tx, name)?;
    }
    if !schema.searchable_fields.is_empty() {
        let field_refs: Vec<&str> = schema
            .searchable_fields
            .iter()
            .map(|s| s.as_str())
            .collect();
        fts::create_fts_index(tx, effective_name, &field_refs)?;
    }

    info!(collection = %schema.name, "Updated collection");
    Ok(())
}

fn alter_collection_impl(
    tx: &Transaction<'_>,
    name: &str,
    alteration: &SchemaAlteration,
) -> Result<()> {
    let schema = &alteration.schema;

    // Look up the existing collection.
    let (coll_id, old_type): (String, String) = tx
        .query_row(
            "SELECT id, type FROM _collections WHERE name = ?1",
            params![name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => DbError::not_found_with_id("Collection", name),
            other => DbError::Query(other),
        })?;

    // If the name changed, check the new name isn't taken.
    if schema.name != name {
        let name_taken: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM _collections WHERE name = ?1)",
                params![schema.name],
                |row| row.get(0),
            )
            .map_err(DbError::Query)?;

        if name_taken {
            return Err(DbError::conflict(format!(
                "collection '{}' already exists",
                schema.name
            )));
        }
    }

    // Validate rename mappings: old names must exist, new names must be in the schema.
    let old_columns = load_columns_from_tx(tx, &coll_id)?;
    let old_col_names: std::collections::HashSet<&str> =
        old_columns.iter().map(|c| c.name.as_str()).collect();
    let new_col_names: std::collections::HashSet<&str> =
        schema.columns.iter().map(|c| c.name.as_str()).collect();

    for (old_name, new_name) in &alteration.renames {
        if !old_col_names.contains(old_name.as_str()) {
            return Err(DbError::schema(format!(
                "cannot rename field '{}': does not exist in collection '{}'",
                old_name, name
            )));
        }
        if !new_col_names.contains(new_name.as_str()) {
            return Err(DbError::schema(format!(
                "rename target '{}' not found in the new schema",
                new_name
            )));
        }
    }

    // Update _collections metadata.
    tx.execute(
        "UPDATE _collections SET name = ?1, type = ?2, updated = datetime('now') WHERE id = ?3",
        params![schema.name, schema.collection_type, coll_id],
    )
    .map_err(DbError::Query)?;

    // Replace all fields: delete old, insert new.
    tx.execute(
        "DELETE FROM _fields WHERE collection_id = ?1",
        params![coll_id],
    )
    .map_err(DbError::Query)?;
    insert_fields(tx, &coll_id, &schema.columns)?;

    // Alter the user table with rename-aware rebuild.
    alter_user_table_with_renames(
        tx,
        name,
        schema,
        &old_columns,
        &old_type,
        &alteration.renames,
    )?;

    info!(collection = %schema.name, "Altered collection");
    Ok(())
}

/// Build the column mapping for the data-copy step of a table rebuild.
///
/// Returns pairs of `(new_column_expr, new_column_name)` where the expression
/// may include a CAST or a column rename.
fn build_copy_expressions(
    old_columns: &[ColumnDef],
    new_columns: &[ColumnDef],
    renames: &[(String, String)],
) -> Vec<(String, String)> {
    let rename_map: std::collections::HashMap<&str, &str> = renames
        .iter()
        .map(|(old, new)| (old.as_str(), new.as_str()))
        .collect();

    // Reverse map: new_name → old_name
    let reverse_rename: std::collections::HashMap<&str, &str> = renames
        .iter()
        .map(|(old, new)| (new.as_str(), old.as_str()))
        .collect();

    let old_by_name: std::collections::HashMap<&str, &ColumnDef> =
        old_columns.iter().map(|c| (c.name.as_str(), c)).collect();

    let mut exprs = Vec::new();

    for new_col in new_columns {
        // Determine which old column this new column corresponds to.
        let source_name = if let Some(&old_name) = reverse_rename.get(new_col.name.as_str()) {
            // This new column was renamed from old_name.
            Some(old_name)
        } else if old_by_name.contains_key(new_col.name.as_str())
            && !rename_map.contains_key(new_col.name.as_str())
        {
            // Same name exists in old schema and wasn't renamed away.
            Some(new_col.name.as_str())
        } else {
            // Truly new column — no source data.
            None
        };

        if let Some(src) = source_name {
            if let Some(old_col) = old_by_name.get(src) {
                let expr = if old_col.sql_type != new_col.sql_type {
                    // Type changed — apply CAST for data migration.
                    build_cast_expression(src, &old_col.sql_type, &new_col.sql_type)
                } else {
                    format!("\"{}\"", src)
                };
                exprs.push((expr, new_col.name.clone()));
            }
        }
        // If no source, the column is new; it gets its DEFAULT or NULL.
    }

    exprs
}

/// Build a SQL expression that converts data from one SQLite type to another.
///
/// Uses CASE expressions rather than bare CAST to handle incompatible values
/// gracefully — returning NULL for values that cannot be converted.
fn build_cast_expression(column: &str, _from_type: &str, to_type: &str) -> String {
    match to_type.to_uppercase().as_str() {
        "INTEGER" => {
            // Try to cast; NULL for non-numeric values.
            format!(
                "CASE WHEN typeof(\"{col}\") IN ('integer','real') THEN CAST(\"{col}\" AS INTEGER) \
                 WHEN typeof(\"{col}\") = 'text' AND \"{col}\" GLOB '[0-9]*' THEN CAST(\"{col}\" AS INTEGER) \
                 ELSE NULL END",
                col = column
            )
        }
        "REAL" => {
            format!(
                "CASE WHEN typeof(\"{col}\") IN ('integer','real') THEN CAST(\"{col}\" AS REAL) \
                 WHEN typeof(\"{col}\") = 'text' AND \"{col}\" GLOB '[0-9]*' THEN CAST(\"{col}\" AS REAL) \
                 WHEN typeof(\"{col}\") = 'text' AND \"{col}\" GLOB '[0-9]*.*' THEN CAST(\"{col}\" AS REAL) \
                 ELSE NULL END",
                col = column
            )
        }
        "TEXT" => {
            // Everything can become TEXT.
            format!("CAST(\"{}\" AS TEXT)", column)
        }
        _ => {
            // For BLOB, NUMERIC, or unknown types — pass through with CAST.
            format!("CAST(\"{}\" AS {})", column, to_type)
        }
    }
}

/// Alter a user table with explicit rename support.
///
/// This is the rename-aware version of `alter_user_table`. It uses the
/// rename mappings to correctly copy data from old column names to new ones,
/// and applies type conversion expressions when the SQL type changes.
fn alter_user_table_with_renames(
    tx: &Transaction<'_>,
    old_name: &str,
    schema: &CollectionSchema,
    old_columns: &[ColumnDef],
    old_type: &str,
    renames: &[(String, String)],
) -> Result<()> {
    let new_name = &schema.name;
    let temp_name = format!("_zb_rebuild_{new_name}");

    // Build the new table under a temporary name.
    let temp_schema = CollectionSchema {
        name: temp_name.clone(),
        collection_type: schema.collection_type.clone(),
        columns: schema.columns.clone(),
        indexes: Vec::new(), // Indexes added after rename.
        searchable_fields: vec![],
        view_query: None,
    };
    create_user_table(tx, &temp_schema)?;

    // Build column copy expressions (handles renames and type casts).
    let user_copy_exprs = build_copy_expressions(old_columns, &schema.columns, renames);

    // Always copy system columns.
    let mut select_parts: Vec<String> = vec!["\"id\"".to_string()];
    let mut insert_cols: Vec<String> = vec!["\"id\"".to_string()];

    // Copy auth system columns if both old and new are auth type.
    if old_type == "auth" && schema.collection_type == "auth" {
        for sys_col in &[
            "email",
            "emailVisibility",
            "verified",
            "password",
            "tokenKey",
        ] {
            select_parts.push(format!("\"{}\"", sys_col));
            insert_cols.push(format!("\"{}\"", sys_col));
        }
    }

    // Add user-defined column expressions.
    for (expr, col_name) in &user_copy_exprs {
        select_parts.push(expr.clone());
        insert_cols.push(format!("\"{}\"", col_name));
    }

    // Always copy timestamp columns.
    select_parts.push("\"created\"".to_string());
    insert_cols.push("\"created\"".to_string());
    select_parts.push("\"updated\"".to_string());
    insert_cols.push("\"updated\"".to_string());

    let insert_csv = insert_cols.join(", ");
    let select_csv = select_parts.join(", ");

    let copy_sql = format!(
        "INSERT INTO \"{temp_name}\" ({insert_csv}) SELECT {select_csv} FROM \"{old_name}\""
    );
    tx.execute_batch(&copy_sql).map_err(DbError::Query)?;

    // Drop old table.
    tx.execute_batch(&format!("DROP TABLE \"{old_name}\""))
        .map_err(DbError::Query)?;

    // Rename temp to final name.
    tx.execute_batch(&format!(
        "ALTER TABLE \"{temp_name}\" RENAME TO \"{new_name}\""
    ))
    .map_err(DbError::Query)?;

    // Recreate system indexes.
    create_index(
        tx,
        new_name,
        &IndexDef {
            name: format!("idx_{}_created", new_name),
            columns: vec!["created".to_string()],
            index_columns: vec![],
            unique: false,
        },
    )?;

    if schema.collection_type == "auth" {
        create_index(
            tx,
            new_name,
            &IndexDef {
                name: format!("idx_{}_email", new_name),
                columns: vec!["email".to_string()],
                index_columns: vec![],
                unique: true,
            },
        )?;
        create_index(
            tx,
            new_name,
            &IndexDef {
                name: format!("idx_{}_tokenKey", new_name),
                columns: vec!["tokenKey".to_string()],
                index_columns: vec![],
                unique: false,
            },
        )?;
    }

    // Recreate user-defined indexes.
    for idx in &schema.indexes {
        create_index(tx, new_name, idx)?;
    }

    debug!(table = %new_name, "Rebuilt user table with renames");
    Ok(())
}

fn delete_collection_impl(tx: &Transaction<'_>, name: &str) -> Result<()> {
    // Look up collection (and verify it exists).
    let (coll_id, coll_type): (String, String) = tx
        .query_row(
            "SELECT id, type FROM _collections WHERE name = ?1",
            params![name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => DbError::not_found_with_id("Collection", name),
            other => DbError::Query(other),
        })?;

    // Fields are cascade-deleted by foreign key constraint.
    tx.execute("DELETE FROM _collections WHERE id = ?1", params![coll_id])
        .map_err(DbError::Query)?;

    if coll_type == "view" {
        // Drop the SQL VIEW.
        drop_view(tx, name)?;
    } else {
        // Drop FTS index if it exists.
        if fts::fts_table_exists(tx, name).unwrap_or(false) {
            fts::drop_fts_index(tx, name)?;
        }
        // Drop the user table and its indexes.
        drop_user_table(tx, name)?;
    }

    info!(collection = %name, "Deleted collection");
    Ok(())
}

fn collection_exists_impl(conn: &Connection, name: &str) -> Result<bool> {
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM _collections WHERE name = ?1)",
            params![name],
            |row| row.get(0),
        )
        .map_err(DbError::Query)?;
    Ok(exists)
}

// ── Field helpers ─────────────────────────────────────────────────────────────

fn insert_fields(tx: &Transaction<'_>, coll_id: &str, columns: &[ColumnDef]) -> Result<()> {
    let mut stmt = tx
        .prepare(
            "INSERT INTO _fields (id, collection_id, name, type, required, unique_field, options, sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .map_err(DbError::Query)?;

    for (i, col) in columns.iter().enumerate() {
        let field_id = generate_id();
        let options = serde_json::json!({
            "default": col.default,
        });

        stmt.execute(params![
            field_id,
            coll_id,
            col.name,
            col.sql_type,
            col.not_null as i32,
            col.unique as i32,
            options.to_string(),
            i as i32,
        ])
        .map_err(DbError::Query)?;
    }

    Ok(())
}

fn load_columns(conn: &Connection, coll_id: &str) -> Result<Vec<ColumnDef>> {
    load_columns_inner(conn, coll_id)
}

fn load_columns_from_tx(tx: &Transaction<'_>, coll_id: &str) -> Result<Vec<ColumnDef>> {
    load_columns_inner(tx, coll_id)
}

/// Generic column loader that works with both Connection and Transaction (via Deref).
fn load_columns_inner(conn: &Connection, coll_id: &str) -> Result<Vec<ColumnDef>> {
    let mut stmt = conn
        .prepare(
            "SELECT name, type, required, unique_field, options
             FROM _fields WHERE collection_id = ?1
             ORDER BY sort_order, rowid",
        )
        .map_err(DbError::Query)?;

    let columns = stmt
        .query_map(params![coll_id], |row| {
            let name: String = row.get(0)?;
            let sql_type: String = row.get(1)?;
            let not_null: bool = row.get::<_, i32>(2)? != 0;
            let unique: bool = row.get::<_, i32>(3)? != 0;
            let options_str: String = row.get(4)?;

            let default = serde_json::from_str::<serde_json::Value>(&options_str)
                .ok()
                .and_then(|v| v.get("default").cloned())
                .and_then(|v| {
                    if v.is_null() {
                        None
                    } else {
                        Some(v.as_str().unwrap_or_default().to_string())
                    }
                });

            Ok(ColumnDef {
                name,
                sql_type,
                not_null,
                default,
                unique,
            })
        })
        .map_err(DbError::Query)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(DbError::Query)?;

    Ok(columns)
}

fn load_indexes(conn: &Connection, table_name: &str) -> Result<Vec<IndexDef>> {
    // Query SQLite's index list for user-defined indexes on this table.
    // Skip autoindex entries and the primary key.
    let mut stmt = conn
        .prepare(
            "SELECT name, sql FROM sqlite_master
             WHERE type = 'index' AND tbl_name = ?1 AND sql IS NOT NULL",
        )
        .map_err(DbError::Query)?;

    let indexes = stmt
        .query_map(params![table_name], |row| {
            let name: String = row.get(0)?;
            let sql: String = row.get(1)?;
            Ok((name, sql))
        })
        .map_err(DbError::Query)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(DbError::Query)?;

    let mut result = Vec::new();
    for (name, sql) in indexes {
        let unique = sql.to_uppercase().contains("UNIQUE");
        // Parse columns from the SQL: ... ("col1" ASC, "col2" DESC, ...)
        if let Some(start) = sql.rfind('(') {
            if let Some(end) = sql.rfind(')') {
                let cols_str = &sql[start + 1..end];
                let mut columns = Vec::new();
                let mut index_columns = Vec::new();
                for part in cols_str.split(',') {
                    let part = part.trim();
                    let upper = part.to_uppercase();
                    let has_desc = upper.ends_with(" DESC");
                    let has_asc = upper.ends_with(" ASC");
                    let col_name = if has_desc || has_asc {
                        // Strip the trailing direction keyword
                        let trimmed = part[..part.len() - 4].trim();
                        trimmed.trim_matches('"').to_string()
                    } else {
                        part.trim_matches('"').to_string()
                    };
                    let sort = if has_desc {
                        IndexColumnSort::Desc
                    } else {
                        IndexColumnSort::Asc
                    };
                    columns.push(col_name.clone());
                    index_columns.push(IndexColumnDef {
                        name: col_name,
                        sort,
                    });
                }
                result.push(IndexDef {
                    name,
                    columns,
                    index_columns,
                    unique,
                });
            }
        }
    }

    Ok(result)
}

// ── DDL helpers ───────────────────────────────────────────────────────────────

fn create_user_table(tx: &Transaction<'_>, schema: &CollectionSchema) -> Result<()> {
    let mut sql = format!(
        "CREATE TABLE \"{}\" (\n  id TEXT PRIMARY KEY NOT NULL,\n",
        schema.name
    );

    // For auth collections, add system auth columns before user columns.
    if schema.collection_type == "auth" {
        sql.push_str("  email TEXT NOT NULL DEFAULT '' UNIQUE,\n");
        sql.push_str("  emailVisibility INTEGER NOT NULL DEFAULT 0,\n");
        sql.push_str("  verified INTEGER NOT NULL DEFAULT 0,\n");
        sql.push_str("  password TEXT NOT NULL DEFAULT '',\n");
        sql.push_str("  tokenKey TEXT NOT NULL DEFAULT '',\n");
    }

    for col in &schema.columns {
        sql.push_str(&format!("  \"{}\" {}", col.name, col.sql_type));
        if col.not_null {
            sql.push_str(" NOT NULL");
        }
        if col.unique {
            sql.push_str(" UNIQUE");
        }
        if let Some(ref default) = col.default {
            sql.push_str(&format!(" DEFAULT {default}"));
        }
        sql.push_str(",\n");
    }

    // System timestamp columns.
    sql.push_str("  created TEXT NOT NULL DEFAULT (datetime('now')),\n");
    sql.push_str("  updated TEXT NOT NULL DEFAULT (datetime('now'))\n");
    sql.push_str(")");

    debug!(table = %schema.name, "Creating user table");
    tx.execute_batch(&sql).map_err(DbError::Query)?;

    // Create automatic system indexes on created column.
    create_index(
        tx,
        &schema.name,
        &IndexDef {
            name: format!("idx_{}_created", schema.name),
            columns: vec!["created".to_string()],
            index_columns: vec![],
            unique: false,
        },
    )?;

    // For auth collections, add index on email.
    if schema.collection_type == "auth" {
        create_index(
            tx,
            &schema.name,
            &IndexDef {
                name: format!("idx_{}_email", schema.name),
                columns: vec!["email".to_string()],
                index_columns: vec![],
                unique: true,
            },
        )?;
        create_index(
            tx,
            &schema.name,
            &IndexDef {
                name: format!("idx_{}_tokenKey", schema.name),
                columns: vec!["tokenKey".to_string()],
                index_columns: vec![],
                unique: false,
            },
        )?;
    }

    // Create user-defined indexes.
    for idx in &schema.indexes {
        create_index(tx, &schema.name, idx)?;
    }

    Ok(())
}

/// Alter a user table using SQLite's recommended 12-step rebuild process.
///
/// SQLite does not support DROP COLUMN or full ALTER COLUMN, so we:
/// 1. Create a new table with the desired schema
/// 2. Copy data from the old table (only columns present in both)
/// 3. Drop the old table
/// 4. Rename the new table
/// 5. Recreate indexes
fn alter_user_table(
    tx: &Transaction<'_>,
    old_name: &str,
    schema: &CollectionSchema,
    old_columns: &[ColumnDef],
) -> Result<()> {
    let new_name = &schema.name;
    let temp_name = format!("_zb_rebuild_{new_name}");

    // Build the new table under a temporary name.
    let temp_schema = CollectionSchema {
        name: temp_name.clone(),
        collection_type: schema.collection_type.clone(),
        columns: schema.columns.clone(),
        indexes: Vec::new(), // Indexes added after rename.
        searchable_fields: vec![],
        view_query: None,
    };
    create_user_table(tx, &temp_schema)?;

    // Find columns present in both old and new schemas for data copy.
    let new_col_names: std::collections::HashSet<&str> =
        schema.columns.iter().map(|c| c.name.as_str()).collect();
    let old_col_names: std::collections::HashSet<&str> =
        old_columns.iter().map(|c| c.name.as_str()).collect();

    let mut common_cols: Vec<&str> = new_col_names
        .intersection(&old_col_names)
        .copied()
        .collect();
    common_cols.sort(); // deterministic order

    // Always copy system columns.
    let mut copy_cols = vec!["id"];

    // Copy auth system columns if the collection type is auth.
    if schema.collection_type == "auth" {
        for sys_col in &[
            "email",
            "emailVisibility",
            "verified",
            "password",
            "tokenKey",
        ] {
            copy_cols.push(sys_col);
        }
    }

    for col in &common_cols {
        if !copy_cols.contains(col) {
            copy_cols.push(col);
        }
    }

    // Timestamp columns always come last.
    copy_cols.push("created");
    copy_cols.push("updated");

    let cols_csv = copy_cols
        .iter()
        .map(|c| format!("\"{}\"", c))
        .collect::<Vec<_>>()
        .join(", ");

    let copy_sql =
        format!("INSERT INTO \"{temp_name}\" ({cols_csv}) SELECT {cols_csv} FROM \"{old_name}\"");
    tx.execute_batch(&copy_sql).map_err(DbError::Query)?;

    // Drop old table.
    tx.execute_batch(&format!("DROP TABLE \"{old_name}\""))
        .map_err(DbError::Query)?;

    // Rename temp to final name.
    tx.execute_batch(&format!(
        "ALTER TABLE \"{temp_name}\" RENAME TO \"{new_name}\""
    ))
    .map_err(DbError::Query)?;

    // Recreate system indexes.
    create_index(
        tx,
        new_name,
        &IndexDef {
            name: format!("idx_{}_created", new_name),
            columns: vec!["created".to_string()],
            index_columns: vec![],
            unique: false,
        },
    )?;

    if schema.collection_type == "auth" {
        create_index(
            tx,
            new_name,
            &IndexDef {
                name: format!("idx_{}_email", new_name),
                columns: vec!["email".to_string()],
                index_columns: vec![],
                unique: true,
            },
        )?;
        create_index(
            tx,
            new_name,
            &IndexDef {
                name: format!("idx_{}_tokenKey", new_name),
                columns: vec!["tokenKey".to_string()],
                index_columns: vec![],
                unique: false,
            },
        )?;
    }

    // Recreate user-defined indexes.
    for idx in &schema.indexes {
        create_index(tx, new_name, idx)?;
    }

    debug!(table = %new_name, "Rebuilt user table");
    Ok(())
}

fn drop_user_table(tx: &Transaction<'_>, name: &str) -> Result<()> {
    tx.execute_batch(&format!("DROP TABLE IF EXISTS \"{name}\""))
        .map_err(DbError::Query)?;
    debug!(table = %name, "Dropped user table");
    Ok(())
}

// ── View helpers ──────────────────────────────────────────────────────────────

fn create_view(tx: &Transaction<'_>, name: &str, query: &str) -> Result<()> {
    // Validate the query by preparing it first (catches syntax errors).
    tx.prepare(query).map_err(|e| {
        DbError::schema(format!(
            "invalid view query for '{}': {}",
            name, e
        ))
    })?;

    let sql = format!("CREATE VIEW \"{}\" AS {}", name, query);
    tx.execute_batch(&sql).map_err(|e| {
        DbError::schema(format!(
            "failed to create view '{}': {}",
            name, e
        ))
    })?;
    debug!(view = %name, "Created SQL VIEW");
    Ok(())
}

fn drop_view(tx: &Transaction<'_>, name: &str) -> Result<()> {
    tx.execute_batch(&format!("DROP VIEW IF EXISTS \"{name}\""))
        .map_err(DbError::Query)?;
    debug!(view = %name, "Dropped SQL VIEW");
    Ok(())
}

/// Infer column definitions from a SQL VIEW by querying PRAGMA table_info.
fn infer_view_columns(conn: &Connection, view_name: &str) -> Result<Vec<ColumnDef>> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info(\"{}\")", view_name))
        .map_err(DbError::Query)?;

    let columns = stmt
        .query_map([], |row| {
            let name: String = row.get(1)?;
            let sql_type: String = row.get(2)?;
            let not_null: bool = row.get(3)?;
            let default: Option<String> = row.get(4)?;
            // pk column (index 5) — views don't have real PKs.
            Ok(ColumnDef {
                name,
                sql_type,
                not_null,
                default,
                unique: false,
            })
        })
        .map_err(DbError::Query)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(DbError::Query)?;

    // Filter out system columns that are part of the underlying tables.
    let columns = columns
        .into_iter()
        .filter(|c| !matches!(c.name.as_str(), "id" | "created" | "updated"))
        .collect();

    Ok(columns)
}

fn create_index(tx: &Transaction<'_>, table: &str, idx: &IndexDef) -> Result<()> {
    let unique = if idx.unique { "UNIQUE " } else { "" };
    let cols = idx.column_exprs().join(", ");

    let sql = format!(
        "CREATE {unique}INDEX IF NOT EXISTS \"{}\" ON \"{table}\" ({cols})",
        idx.name
    );
    tx.execute_batch(&sql).map_err(DbError::Query)?;
    Ok(())
}

// ── Utilities ─────────────────────────────────────────────────────────────────

// ── Core SchemaRepository bridge ──────────────────────────────────────────────
//
// The core crate defines its own `SchemaRepository` trait using DTO types that
// mirror the DB layer's types. This impl bridges the two so `Database` can be
// used directly with `CollectionService`.

mod core_schema_bridge {
    use super::*;
    use zerobase_core::services::collection_service::{
        CollectionSchemaDto, ColumnDto, IndexColumnDto, IndexColumnSortDto, IndexDto,
        SchemaRepoError, SchemaRepository as CoreSchemaRepository,
    };

    fn db_to_dto(schema: &CollectionSchema) -> CollectionSchemaDto {
        CollectionSchemaDto {
            name: schema.name.clone(),
            collection_type: schema.collection_type.clone(),
            columns: schema
                .columns
                .iter()
                .map(|c| ColumnDto {
                    name: c.name.clone(),
                    sql_type: c.sql_type.clone(),
                    not_null: c.not_null,
                    default: c.default.clone(),
                    unique: c.unique,
                })
                .collect(),
            indexes: schema
                .indexes
                .iter()
                .map(|i| IndexDto {
                    name: i.name.clone(),
                    columns: i.columns.clone(),
                    index_columns: i
                        .index_columns
                        .iter()
                        .map(|ic| IndexColumnDto {
                            name: ic.name.clone(),
                            sort: match ic.sort {
                                IndexColumnSort::Asc => IndexColumnSortDto::Asc,
                                IndexColumnSort::Desc => IndexColumnSortDto::Desc,
                            },
                        })
                        .collect(),
                    unique: i.unique,
                })
                .collect(),
            searchable_fields: schema.searchable_fields.clone(),
            view_query: schema.view_query.clone(),
        }
    }

    fn dto_to_db(dto: &CollectionSchemaDto) -> CollectionSchema {
        CollectionSchema {
            name: dto.name.clone(),
            collection_type: dto.collection_type.clone(),
            columns: dto
                .columns
                .iter()
                .map(|c| ColumnDef {
                    name: c.name.clone(),
                    sql_type: c.sql_type.clone(),
                    not_null: c.not_null,
                    default: c.default.clone(),
                    unique: c.unique,
                })
                .collect(),
            indexes: dto
                .indexes
                .iter()
                .map(|i| IndexDef {
                    name: i.name.clone(),
                    columns: i.columns.clone(),
                    index_columns: i
                        .index_columns
                        .iter()
                        .map(|ic| IndexColumnDef {
                            name: ic.name.clone(),
                            sort: match ic.sort {
                                IndexColumnSortDto::Asc => IndexColumnSort::Asc,
                                IndexColumnSortDto::Desc => IndexColumnSort::Desc,
                            },
                        })
                        .collect(),
                    unique: i.unique,
                })
                .collect(),
            searchable_fields: dto.searchable_fields.clone(),
            view_query: dto.view_query.clone(),
        }
    }

    fn to_schema_repo_error(e: crate::error::DbError) -> SchemaRepoError {
        match e {
            DbError::NotFound {
                resource_type,
                resource_id,
            } => SchemaRepoError::NotFound {
                resource_type,
                resource_id,
            },
            DbError::Conflict { message } => SchemaRepoError::Conflict { message },
            DbError::Schema { message } => SchemaRepoError::Schema { message },
            other => SchemaRepoError::Database {
                message: other.to_string(),
            },
        }
    }

    impl CoreSchemaRepository for Database {
        fn list_collections(
            &self,
        ) -> std::result::Result<Vec<CollectionSchemaDto>, SchemaRepoError> {
            SchemaRepository::list_collections(self)
                .map(|v| v.iter().map(db_to_dto).collect())
                .map_err(to_schema_repo_error)
        }

        fn get_collection(
            &self,
            name: &str,
        ) -> std::result::Result<CollectionSchemaDto, SchemaRepoError> {
            SchemaRepository::get_collection(self, name)
                .map(|s| db_to_dto(&s))
                .map_err(to_schema_repo_error)
        }

        fn create_collection(
            &self,
            schema: &CollectionSchemaDto,
        ) -> std::result::Result<(), SchemaRepoError> {
            SchemaRepository::create_collection(self, &dto_to_db(schema))
                .map_err(to_schema_repo_error)
        }

        fn update_collection(
            &self,
            name: &str,
            schema: &CollectionSchemaDto,
        ) -> std::result::Result<(), SchemaRepoError> {
            SchemaRepository::update_collection(self, name, &dto_to_db(schema))
                .map_err(to_schema_repo_error)
        }

        fn delete_collection(&self, name: &str) -> std::result::Result<(), SchemaRepoError> {
            SchemaRepository::delete_collection(self, name).map_err(to_schema_repo_error)
        }

        fn collection_exists(&self, name: &str) -> std::result::Result<bool, SchemaRepoError> {
            SchemaRepository::collection_exists(self, name).map_err(to_schema_repo_error)
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::PoolConfig;

    fn setup_db() -> Database {
        let db = Database::open_in_memory(&PoolConfig::default()).unwrap();
        db.run_system_migrations().unwrap();
        db
    }

    fn text_column(name: &str) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            sql_type: "TEXT".to_string(),
            not_null: false,
            default: None,
            unique: false,
        }
    }

    fn required_text_column(name: &str) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            sql_type: "TEXT".to_string(),
            not_null: true,
            default: None,
            unique: false,
        }
    }

    fn real_column(name: &str) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            sql_type: "REAL".to_string(),
            not_null: false,
            default: None,
            unique: false,
        }
    }

    fn integer_column(name: &str) -> ColumnDef {
        ColumnDef {
            name: name.to_string(),
            sql_type: "INTEGER".to_string(),
            not_null: false,
            default: None,
            unique: false,
        }
    }

    fn base_schema(name: &str, columns: Vec<ColumnDef>) -> CollectionSchema {
        CollectionSchema {
            name: name.to_string(),
            collection_type: "base".to_string(),
            columns,
            indexes: Vec::new(),
            searchable_fields: vec![],
            view_query: None,
        }
    }

    fn auth_schema(name: &str, columns: Vec<ColumnDef>) -> CollectionSchema {
        CollectionSchema {
            name: name.to_string(),
            collection_type: "auth".to_string(),
            columns,
            indexes: Vec::new(),
            searchable_fields: vec![],
            view_query: None,
        }
    }

    /// Helper: returns all column names for a table via PRAGMA table_info.
    fn table_columns(db: &Database, table: &str) -> Vec<(String, String, bool)> {
        let conn = db.read_conn().unwrap();
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info(\"{}\")", table))
            .unwrap();
        stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,   // name
                row.get::<_, String>(2)?,   // type
                row.get::<_, i32>(3)? != 0, // notnull
            ))
        })
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap()
    }

    /// Helper: returns all index names for a table.
    fn table_indexes(db: &Database, table: &str) -> Vec<(String, bool)> {
        let conn = db.read_conn().unwrap();
        let mut stmt = conn
            .prepare(&format!("PRAGMA index_list(\"{}\")", table))
            .unwrap();
        stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,   // name
                row.get::<_, i32>(2)? != 0, // unique
            ))
        })
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap()
    }

    // ── create_collection ─────────────────────────────────────────────────

    #[test]
    fn create_and_get_collection() {
        let db = setup_db();
        let schema = base_schema(
            "posts",
            vec![required_text_column("title"), text_column("body")],
        );

        db.create_collection(&schema).unwrap();

        let retrieved = db.get_collection("posts").unwrap();
        assert_eq!(retrieved.name, "posts");
        assert_eq!(retrieved.collection_type, "base");
        assert_eq!(retrieved.columns.len(), 2);
        assert_eq!(retrieved.columns[0].name, "title");
        assert!(retrieved.columns[0].not_null);
        assert_eq!(retrieved.columns[1].name, "body");
        assert!(!retrieved.columns[1].not_null);
    }

    #[test]
    fn create_collection_creates_sqlite_table() {
        let db = setup_db();
        let schema = base_schema("tasks", vec![text_column("description")]);
        db.create_collection(&schema).unwrap();

        // Verify the table exists by inserting a row.
        db.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO tasks (id, description) VALUES ('abc', 'test task')",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let conn = db.read_conn().unwrap();
        let desc: String = conn
            .query_row("SELECT description FROM tasks WHERE id = 'abc'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(desc, "test task");
    }

    #[test]
    fn create_duplicate_collection_fails() {
        let db = setup_db();
        let schema = base_schema("posts", vec![text_column("title")]);
        db.create_collection(&schema).unwrap();

        let result = db.create_collection(&schema);
        assert!(result.is_err());
        match result.unwrap_err() {
            DbError::Conflict { message } => {
                assert!(message.contains("already exists"));
            }
            other => panic!("expected Conflict, got: {other:?}"),
        }
    }

    #[test]
    fn create_collection_with_indexes() {
        let db = setup_db();
        let schema = CollectionSchema {
            name: "articles".to_string(),
            collection_type: "base".to_string(),
            columns: vec![required_text_column("title"), text_column("slug")],
            indexes: vec![IndexDef {
                name: "idx_articles_slug".to_string(),
                columns: vec!["slug".to_string()],
                index_columns: vec![],
                unique: true,
            }],
            searchable_fields: vec![],
            view_query: None,
        };
        db.create_collection(&schema).unwrap();

        let retrieved = db.get_collection("articles").unwrap();
        // User-defined index plus auto-generated created index.
        assert!(retrieved
            .indexes
            .iter()
            .any(|i| i.name == "idx_articles_slug"));
        assert!(
            retrieved.indexes[0].unique
                || retrieved
                    .indexes
                    .iter()
                    .any(|i| i.name == "idx_articles_slug" && i.unique)
        );
    }

    #[test]
    fn create_collection_with_unique_column() {
        let db = setup_db();
        let schema = auth_schema(
            "users",
            vec![ColumnDef {
                name: "display_name".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: true,
                default: None,
                unique: true,
            }],
        );
        db.create_collection(&schema).unwrap();

        // Insert one row — auth table has email column, so fill it.
        db.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO users (id, email, display_name) VALUES ('u1', 'a@b.com', 'Alice')",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        // Insert duplicate display_name should fail.
        let result = db.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO users (id, email, display_name) VALUES ('u2', 'b@b.com', 'Alice')",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        });
        assert!(result.is_err());
    }

    // ── list_collections ──────────────────────────────────────────────────

    #[test]
    fn list_collections_empty() {
        let db = setup_db();
        let collections = db.list_collections().unwrap();
        assert!(collections.is_empty());
    }

    #[test]
    fn list_collections_returns_all() {
        let db = setup_db();
        db.create_collection(&base_schema("alpha", vec![text_column("a")]))
            .unwrap();
        db.create_collection(&base_schema("beta", vec![text_column("b")]))
            .unwrap();
        db.create_collection(&base_schema("gamma", vec![text_column("c")]))
            .unwrap();

        let collections = db.list_collections().unwrap();
        assert_eq!(collections.len(), 3);
        // Should be sorted by name.
        assert_eq!(collections[0].name, "alpha");
        assert_eq!(collections[1].name, "beta");
        assert_eq!(collections[2].name, "gamma");
    }

    // ── get_collection ────────────────────────────────────────────────────

    #[test]
    fn get_nonexistent_collection_fails() {
        let db = setup_db();
        let result = db.get_collection("nonexistent");
        assert!(result.is_err());
        match result.unwrap_err() {
            DbError::NotFound { resource_type, .. } => {
                assert_eq!(resource_type, "Collection");
            }
            other => panic!("expected NotFound, got: {other:?}"),
        }
    }

    // ── collection_exists ─────────────────────────────────────────────────

    #[test]
    fn collection_exists_returns_correct_value() {
        let db = setup_db();
        assert!(!db.collection_exists("posts").unwrap());

        db.create_collection(&base_schema("posts", vec![text_column("title")]))
            .unwrap();
        assert!(db.collection_exists("posts").unwrap());
    }

    // ── update_collection ─────────────────────────────────────────────────

    #[test]
    fn update_collection_adds_column() {
        let db = setup_db();
        let schema = base_schema("posts", vec![required_text_column("title")]);
        db.create_collection(&schema).unwrap();

        // Insert a row before update.
        db.with_write_conn(|conn| {
            conn.execute("INSERT INTO posts (id, title) VALUES ('p1', 'Hello')", [])
                .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        // Update: add a "body" column.
        let updated = CollectionSchema {
            name: "posts".to_string(),
            collection_type: "base".to_string(),
            columns: vec![required_text_column("title"), text_column("body")],
            indexes: Vec::new(),
            searchable_fields: vec![],
            view_query: None,
        };
        db.update_collection("posts", &updated).unwrap();

        // Verify the column exists and old data preserved.
        let conn = db.read_conn().unwrap();
        let (title, body): (String, Option<String>) = conn
            .query_row("SELECT title, body FROM posts WHERE id = 'p1'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(title, "Hello");
        assert!(body.is_none());

        // Metadata updated.
        let retrieved = db.get_collection("posts").unwrap();
        assert_eq!(retrieved.columns.len(), 2);
    }

    #[test]
    fn update_collection_removes_column() {
        let db = setup_db();
        let schema = base_schema(
            "posts",
            vec![required_text_column("title"), text_column("body")],
        );
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO posts (id, title, body) VALUES ('p1', 'Hello', 'World')",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        // Update: remove "body" column.
        let updated = base_schema("posts", vec![required_text_column("title")]);
        db.update_collection("posts", &updated).unwrap();

        // Verify "body" column no longer exists.
        let conn = db.read_conn().unwrap();
        let result = conn.query_row("SELECT body FROM posts WHERE id = 'p1'", [], |r| {
            r.get::<_, String>(0)
        });
        assert!(result.is_err()); // column doesn't exist

        // Title data preserved.
        let title: String = conn
            .query_row("SELECT title FROM posts WHERE id = 'p1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(title, "Hello");
    }

    #[test]
    fn update_collection_renames_table() {
        let db = setup_db();
        let schema = base_schema("old_name", vec![text_column("title")]);
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute("INSERT INTO old_name (id, title) VALUES ('r1', 'Test')", [])
                .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let updated = base_schema("new_name", vec![text_column("title")]);
        db.update_collection("old_name", &updated).unwrap();

        // Old name should not exist.
        assert!(!db.collection_exists("old_name").unwrap());
        assert!(db.collection_exists("new_name").unwrap());

        // Data preserved under new name.
        let conn = db.read_conn().unwrap();
        let title: String = conn
            .query_row("SELECT title FROM new_name WHERE id = 'r1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(title, "Test");
    }

    #[test]
    fn update_nonexistent_collection_fails() {
        let db = setup_db();
        let schema = base_schema("ghost", vec![text_column("x")]);
        let result = db.update_collection("ghost", &schema);
        assert!(result.is_err());
        match result.unwrap_err() {
            DbError::NotFound { .. } => {}
            other => panic!("expected NotFound, got: {other:?}"),
        }
    }

    #[test]
    fn update_collection_rename_conflicts() {
        let db = setup_db();
        db.create_collection(&base_schema("alpha", vec![text_column("a")]))
            .unwrap();
        db.create_collection(&base_schema("beta", vec![text_column("b")]))
            .unwrap();

        // Try renaming alpha to beta.
        let updated = base_schema("beta", vec![text_column("a")]);
        let result = db.update_collection("alpha", &updated);
        assert!(result.is_err());
        match result.unwrap_err() {
            DbError::Conflict { .. } => {}
            other => panic!("expected Conflict, got: {other:?}"),
        }
    }

    // ── delete_collection ─────────────────────────────────────────────────

    #[test]
    fn delete_collection_removes_metadata_and_table() {
        let db = setup_db();
        db.create_collection(&base_schema("posts", vec![text_column("title")]))
            .unwrap();
        assert!(db.collection_exists("posts").unwrap());

        db.delete_collection("posts").unwrap();
        assert!(!db.collection_exists("posts").unwrap());

        // Table should be gone.
        let conn = db.read_conn().unwrap();
        let result = conn.execute("INSERT INTO posts (id) VALUES ('x')", []);
        assert!(result.is_err());
    }

    #[test]
    fn delete_collection_cascades_fields() {
        let db = setup_db();
        db.create_collection(&base_schema(
            "posts",
            vec![text_column("title"), text_column("body")],
        ))
        .unwrap();

        // Verify fields exist.
        let conn = db.read_conn().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM _fields WHERE collection_id = (SELECT id FROM _collections WHERE name = 'posts')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);

        db.delete_collection("posts").unwrap();

        // Fields should be cascade-deleted.
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM _fields", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn delete_nonexistent_collection_fails() {
        let db = setup_db();
        let result = db.delete_collection("nonexistent");
        assert!(result.is_err());
        match result.unwrap_err() {
            DbError::NotFound { .. } => {}
            other => panic!("expected NotFound, got: {other:?}"),
        }
    }

    // ── System columns ────────────────────────────────────────────────────

    #[test]
    fn created_and_updated_columns_are_auto_set() {
        let db = setup_db();
        db.create_collection(&base_schema("notes", vec![text_column("text")]))
            .unwrap();

        db.with_write_conn(|conn| {
            conn.execute("INSERT INTO notes (id, text) VALUES ('n1', 'hello')", [])
                .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let conn = db.read_conn().unwrap();
        let (created, updated): (String, String) = conn
            .query_row(
                "SELECT created, updated FROM notes WHERE id = 'n1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();

        assert!(!created.is_empty());
        assert!(!updated.is_empty());
    }

    #[test]
    fn base_table_has_system_columns_id_created_updated() {
        let db = setup_db();
        db.create_collection(&base_schema("items", vec![text_column("label")]))
            .unwrap();

        let cols = table_columns(&db, "items");
        let col_names: Vec<&str> = cols.iter().map(|(n, _, _)| n.as_str()).collect();

        assert!(col_names.contains(&"id"), "missing 'id' column");
        assert!(col_names.contains(&"created"), "missing 'created' column");
        assert!(col_names.contains(&"updated"), "missing 'updated' column");
        assert!(col_names.contains(&"label"), "missing 'label' column");

        // id is TEXT, created is TEXT, updated is TEXT
        let id_col = cols.iter().find(|(n, _, _)| n == "id").unwrap();
        assert_eq!(id_col.1, "TEXT");
        assert!(id_col.2, "id should be NOT NULL");

        let created_col = cols.iter().find(|(n, _, _)| n == "created").unwrap();
        assert_eq!(created_col.1, "TEXT");
        assert!(created_col.2, "created should be NOT NULL");

        let updated_col = cols.iter().find(|(n, _, _)| n == "updated").unwrap();
        assert_eq!(updated_col.1, "TEXT");
        assert!(updated_col.2, "updated should be NOT NULL");
    }

    // ── Field type → SQL column type mapping ──────────────────────────────

    #[test]
    fn text_field_maps_to_text_column() {
        let db = setup_db();
        let schema = base_schema("t", vec![text_column("name")]);
        db.create_collection(&schema).unwrap();

        let cols = table_columns(&db, "t");
        let col = cols.iter().find(|(n, _, _)| n == "name").unwrap();
        assert_eq!(col.1, "TEXT");
    }

    #[test]
    fn number_field_maps_to_real_column() {
        let db = setup_db();
        let schema = base_schema("t", vec![real_column("price")]);
        db.create_collection(&schema).unwrap();

        let cols = table_columns(&db, "t");
        let col = cols.iter().find(|(n, _, _)| n == "price").unwrap();
        assert_eq!(col.1, "REAL");
    }

    #[test]
    fn bool_field_maps_to_integer_column() {
        let db = setup_db();
        let schema = base_schema("t", vec![integer_column("active")]);
        db.create_collection(&schema).unwrap();

        let cols = table_columns(&db, "t");
        let col = cols.iter().find(|(n, _, _)| n == "active").unwrap();
        assert_eq!(col.1, "INTEGER");
    }

    #[test]
    fn datetime_field_maps_to_text_column() {
        let db = setup_db();
        // DateTime, AutoDate, Email, Url, Select, File, Relation, Json, Editor,
        // Password all map to TEXT at the SQLite level.
        let schema = base_schema(
            "events",
            vec![
                text_column("start_at"),   // DateTime → TEXT
                text_column("contact"),    // Email → TEXT
                text_column("website"),    // Url → TEXT
                text_column("category"),   // Select → TEXT
                text_column("attachment"), // File → TEXT
                text_column("parent"),     // Relation → TEXT
                text_column("metadata"),   // Json → TEXT
                text_column("content"),    // Editor → TEXT
            ],
        );
        db.create_collection(&schema).unwrap();

        let cols = table_columns(&db, "events");
        for name in &[
            "start_at",
            "contact",
            "website",
            "category",
            "attachment",
            "parent",
            "metadata",
            "content",
        ] {
            let col = cols.iter().find(|(n, _, _)| n == name).unwrap();
            assert_eq!(col.1, "TEXT", "column '{name}' should be TEXT");
        }
    }

    #[test]
    fn create_collection_with_all_field_types_mixed() {
        let db = setup_db();
        let schema = CollectionSchema {
            name: "products".to_string(),
            collection_type: "base".to_string(),
            columns: vec![
                required_text_column("name"), // Text → TEXT NOT NULL
                ColumnDef {
                    // Number → REAL with default
                    name: "price".to_string(),
                    sql_type: "REAL".to_string(),
                    not_null: true,
                    default: Some("0.0".to_string()),
                    unique: false,
                },
                ColumnDef {
                    // Bool → INTEGER with default
                    name: "active".to_string(),
                    sql_type: "INTEGER".to_string(),
                    not_null: true,
                    default: Some("1".to_string()),
                    unique: false,
                },
                text_column("description"), // Text → TEXT nullable
                text_column("category"),    // Select → TEXT
                text_column("sku"),         // Text → TEXT
            ],
            indexes: vec![IndexDef {
                name: "idx_products_sku".to_string(),
                columns: vec!["sku".to_string()],
                index_columns: vec![],
                unique: true,
            }],
            searchable_fields: vec![],
            view_query: None,
        };
        db.create_collection(&schema).unwrap();

        // Insert a row using defaults for price and active.
        db.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO products (id, name, sku) VALUES ('p1', 'Widget', 'W001')",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let conn = db.read_conn().unwrap();
        let (price, active): (f64, i32) = conn
            .query_row(
                "SELECT price, active FROM products WHERE id = 'p1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!((price - 0.0).abs() < f64::EPSILON);
        assert_eq!(active, 1);

        // Verify column types via PRAGMA.
        let cols = table_columns(&db, "products");
        let price_col = cols.iter().find(|(n, _, _)| n == "price").unwrap();
        assert_eq!(price_col.1, "REAL");
        assert!(price_col.2, "price should be NOT NULL");

        let active_col = cols.iter().find(|(n, _, _)| n == "active").unwrap();
        assert_eq!(active_col.1, "INTEGER");
        assert!(active_col.2, "active should be NOT NULL");
    }

    // ── Auto-generated indexes ────────────────────────────────────────────

    #[test]
    fn base_collection_has_auto_index_on_created() {
        let db = setup_db();
        db.create_collection(&base_schema("posts", vec![text_column("title")]))
            .unwrap();

        let indexes = table_indexes(&db, "posts");
        let idx_names: Vec<&str> = indexes.iter().map(|(n, _)| n.as_str()).collect();
        assert!(
            idx_names.iter().any(|n| n.contains("created")),
            "expected auto-index on created column, found: {:?}",
            idx_names
        );
    }

    #[test]
    fn user_defined_indexes_are_created() {
        let db = setup_db();
        let schema = CollectionSchema {
            name: "articles".to_string(),
            collection_type: "base".to_string(),
            columns: vec![text_column("slug"), text_column("title")],
            indexes: vec![
                IndexDef {
                    name: "idx_articles_slug".to_string(),
                    columns: vec!["slug".to_string()],
                    index_columns: vec![],
                    unique: true,
                },
                IndexDef {
                    name: "idx_articles_title".to_string(),
                    columns: vec!["title".to_string()],
                    index_columns: vec![],
                    unique: false,
                },
            ],
            searchable_fields: vec![],
            view_query: None,
        };
        db.create_collection(&schema).unwrap();

        let indexes = table_indexes(&db, "articles");
        let idx_names: Vec<&str> = indexes.iter().map(|(n, _)| n.as_str()).collect();

        assert!(idx_names.contains(&"idx_articles_slug"));
        assert!(idx_names.contains(&"idx_articles_title"));

        // Verify slug index is unique.
        let slug_idx = indexes
            .iter()
            .find(|(n, _)| n == "idx_articles_slug")
            .unwrap();
        assert!(slug_idx.1, "slug index should be unique");

        // Verify title index is not unique.
        let title_idx = indexes
            .iter()
            .find(|(n, _)| n == "idx_articles_title")
            .unwrap();
        assert!(!title_idx.1, "title index should not be unique");
    }

    // ── Auth collection columns ───────────────────────────────────────────

    #[test]
    fn auth_collection_has_system_auth_columns() {
        let db = setup_db();
        let schema = auth_schema("users", vec![text_column("name")]);
        db.create_collection(&schema).unwrap();

        let cols = table_columns(&db, "users");
        let col_names: Vec<&str> = cols.iter().map(|(n, _, _)| n.as_str()).collect();

        // System columns.
        assert!(col_names.contains(&"id"), "missing 'id'");
        assert!(col_names.contains(&"created"), "missing 'created'");
        assert!(col_names.contains(&"updated"), "missing 'updated'");

        // Auth-specific system columns.
        assert!(col_names.contains(&"email"), "missing 'email'");
        assert!(
            col_names.contains(&"emailVisibility"),
            "missing 'emailVisibility'"
        );
        assert!(col_names.contains(&"verified"), "missing 'verified'");
        assert!(col_names.contains(&"password"), "missing 'password'");
        assert!(col_names.contains(&"tokenKey"), "missing 'tokenKey'");

        // User field.
        assert!(col_names.contains(&"name"), "missing user-defined 'name'");

        // Verify auth column types.
        let email_col = cols.iter().find(|(n, _, _)| n == "email").unwrap();
        assert_eq!(email_col.1, "TEXT");
        assert!(email_col.2, "email should be NOT NULL");

        let verified_col = cols.iter().find(|(n, _, _)| n == "verified").unwrap();
        assert_eq!(verified_col.1, "INTEGER");

        let password_col = cols.iter().find(|(n, _, _)| n == "password").unwrap();
        assert_eq!(password_col.1, "TEXT");
    }

    #[test]
    fn auth_collection_has_email_and_tokenkey_indexes() {
        let db = setup_db();
        let schema = auth_schema("members", vec![text_column("role")]);
        db.create_collection(&schema).unwrap();

        let indexes = table_indexes(&db, "members");
        let idx_names: Vec<&str> = indexes.iter().map(|(n, _)| n.as_str()).collect();

        assert!(
            idx_names.iter().any(|n| n.contains("email")),
            "expected index on email, found: {:?}",
            idx_names
        );
        assert!(
            idx_names.iter().any(|n| n.contains("tokenKey")),
            "expected index on tokenKey, found: {:?}",
            idx_names
        );
        assert!(
            idx_names.iter().any(|n| n.contains("created")),
            "expected index on created, found: {:?}",
            idx_names
        );
    }

    #[test]
    fn auth_collection_enforces_email_uniqueness() {
        let db = setup_db();
        let schema = auth_schema("accounts", vec![]);
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO accounts (id, email, password, tokenKey) VALUES ('a1', 'user@test.com', 'hash1', 'tk1')",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        // Duplicate email should fail.
        let result = db.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO accounts (id, email, password, tokenKey) VALUES ('a2', 'user@test.com', 'hash2', 'tk2')",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        });
        assert!(result.is_err(), "duplicate email should be rejected");
    }

    #[test]
    fn auth_collection_can_insert_and_query() {
        let db = setup_db();
        let schema = auth_schema("users", vec![text_column("name")]);
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO users (id, email, password, tokenKey, name) \
                 VALUES ('u1', 'alice@test.com', '$argon2id$hash', 'tk_abc', 'Alice')",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let conn = db.read_conn().unwrap();
        let (email, name, verified): (String, String, i32) = conn
            .query_row(
                "SELECT email, name, verified FROM users WHERE id = 'u1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(email, "alice@test.com");
        assert_eq!(name, "Alice");
        assert_eq!(verified, 0); // default
    }

    // ── Base collection (no auth columns) ─────────────────────────────────

    #[test]
    fn base_collection_does_not_have_auth_columns() {
        let db = setup_db();
        db.create_collection(&base_schema("posts", vec![text_column("title")]))
            .unwrap();

        let cols = table_columns(&db, "posts");
        let col_names: Vec<&str> = cols.iter().map(|(n, _, _)| n.as_str()).collect();

        assert!(
            !col_names.contains(&"email"),
            "base should not have 'email'"
        );
        assert!(
            !col_names.contains(&"password"),
            "base should not have 'password'"
        );
        assert!(
            !col_names.contains(&"verified"),
            "base should not have 'verified'"
        );
        assert!(
            !col_names.contains(&"tokenKey"),
            "base should not have 'tokenKey'"
        );
    }

    // ── Column constraints ────────────────────────────────────────────────

    #[test]
    fn not_null_constraint_is_enforced() {
        let db = setup_db();
        let schema = base_schema("items", vec![required_text_column("title")]);
        db.create_collection(&schema).unwrap();

        // Attempting to insert without required column should fail.
        let result = db.with_write_conn(|conn| {
            conn.execute("INSERT INTO items (id) VALUES ('i1')", [])
                .map_err(DbError::Query)?;
            Ok(())
        });
        assert!(result.is_err(), "NOT NULL constraint should be enforced");
    }

    #[test]
    fn default_values_are_applied() {
        let db = setup_db();
        let schema = base_schema(
            "flags",
            vec![
                ColumnDef {
                    name: "enabled".to_string(),
                    sql_type: "INTEGER".to_string(),
                    not_null: true,
                    default: Some("0".to_string()),
                    unique: false,
                },
                ColumnDef {
                    name: "label".to_string(),
                    sql_type: "TEXT".to_string(),
                    not_null: true,
                    default: Some("'untitled'".to_string()),
                    unique: false,
                },
            ],
        );
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute("INSERT INTO flags (id) VALUES ('f1')", [])
                .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let conn = db.read_conn().unwrap();
        let (enabled, label): (i32, String) = conn
            .query_row(
                "SELECT enabled, label FROM flags WHERE id = 'f1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(enabled, 0);
        assert_eq!(label, "untitled");
    }

    // ── ID generation ─────────────────────────────────────────────────────

    #[test]
    fn generate_id_produces_15_char_string() {
        let id = generate_id();
        assert_eq!(id.len(), 15, "ID should be 15 characters, got: {id}");
    }

    #[test]
    fn generate_id_is_unique() {
        let ids: Vec<String> = (0..100).map(|_| generate_id()).collect();
        let unique: std::collections::HashSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "IDs should be unique");
    }

    // ── Composite indexes ─────────────────────────────────────────────────

    #[test]
    fn composite_index_is_created() {
        let db = setup_db();
        let schema = CollectionSchema {
            name: "events".to_string(),
            collection_type: "base".to_string(),
            columns: vec![text_column("category"), text_column("date")],
            indexes: vec![IndexDef {
                name: "idx_events_cat_date".to_string(),
                columns: vec!["category".to_string(), "date".to_string()],
                index_columns: vec![],
                unique: false,
            }],
            searchable_fields: vec![],
            view_query: None,
        };
        db.create_collection(&schema).unwrap();

        let indexes = table_indexes(&db, "events");
        assert!(
            indexes.iter().any(|(n, _)| n == "idx_events_cat_date"),
            "composite index should exist"
        );
    }

    // ── Empty columns ─────────────────────────────────────────────────────

    #[test]
    fn create_collection_with_no_user_columns() {
        let db = setup_db();
        let schema = base_schema("empty_table", vec![]);
        db.create_collection(&schema).unwrap();

        // Should still have system columns.
        let cols = table_columns(&db, "empty_table");
        let col_names: Vec<&str> = cols.iter().map(|(n, _, _)| n.as_str()).collect();
        assert!(col_names.contains(&"id"));
        assert!(col_names.contains(&"created"));
        assert!(col_names.contains(&"updated"));
        assert_eq!(cols.len(), 3, "should only have 3 system columns");

        // Should be able to insert.
        db.with_write_conn(|conn| {
            conn.execute("INSERT INTO empty_table (id) VALUES ('e1')", [])
                .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();
    }

    // ── alter_collection — field renaming ──────────────────────────────

    fn alteration(schema: CollectionSchema, renames: Vec<(&str, &str)>) -> crate::SchemaAlteration {
        crate::SchemaAlteration {
            schema,
            renames: renames
                .into_iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
        }
    }

    #[test]
    fn alter_rename_single_field() {
        let db = setup_db();
        let schema = base_schema("posts", vec![text_column("title")]);
        db.create_collection(&schema).unwrap();

        // Insert data.
        db.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO posts (id, title) VALUES ('p1', 'Hello World')",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        // Rename title → headline.
        let new_schema = base_schema("posts", vec![text_column("headline")]);
        db.alter_collection(
            "posts",
            &alteration(new_schema, vec![("title", "headline")]),
        )
        .unwrap();

        // Verify data migrated to new column name.
        let conn = db.read_conn().unwrap();
        let val: String = conn
            .query_row("SELECT headline FROM posts WHERE id = 'p1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(val, "Hello World");

        // Old column should not exist.
        let err = conn.query_row("SELECT title FROM posts WHERE id = 'p1'", [], |r| {
            r.get::<_, String>(0)
        });
        assert!(err.is_err());

        // Metadata updated.
        let retrieved = db.get_collection("posts").unwrap();
        assert_eq!(retrieved.columns.len(), 1);
        assert_eq!(retrieved.columns[0].name, "headline");
    }

    #[test]
    fn alter_rename_multiple_fields() {
        let db = setup_db();
        let schema = base_schema(
            "items",
            vec![
                text_column("name"),
                text_column("desc"),
                real_column("price"),
            ],
        );
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO items (id, name, desc, price) VALUES ('i1', 'Widget', 'A widget', 9.99)",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        // Rename name → label, desc → description.
        let new_schema = base_schema(
            "items",
            vec![
                text_column("label"),
                text_column("description"),
                real_column("price"),
            ],
        );
        db.alter_collection(
            "items",
            &alteration(new_schema, vec![("name", "label"), ("desc", "description")]),
        )
        .unwrap();

        let conn = db.read_conn().unwrap();
        let (label, description, price): (String, String, f64) = conn
            .query_row(
                "SELECT label, description, price FROM items WHERE id = 'i1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(label, "Widget");
        assert_eq!(description, "A widget");
        assert!((price - 9.99).abs() < 0.001);
    }

    #[test]
    fn alter_rename_and_add_field_simultaneously() {
        let db = setup_db();
        let schema = base_schema("notes", vec![text_column("text")]);
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute("INSERT INTO notes (id, text) VALUES ('n1', 'my note')", [])
                .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        // Rename text → content, add a new "tags" column.
        let new_schema = base_schema("notes", vec![text_column("content"), text_column("tags")]);
        db.alter_collection("notes", &alteration(new_schema, vec![("text", "content")]))
            .unwrap();

        let conn = db.read_conn().unwrap();
        let (content, tags): (String, Option<String>) = conn
            .query_row("SELECT content, tags FROM notes WHERE id = 'n1'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(content, "my note");
        assert!(tags.is_none()); // New column, no data.
    }

    #[test]
    fn alter_rename_and_remove_field_simultaneously() {
        let db = setup_db();
        let schema = base_schema(
            "docs",
            vec![
                text_column("title"),
                text_column("body"),
                text_column("draft"),
            ],
        );
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO docs (id, title, body, draft) VALUES ('d1', 'Doc', 'Content', 'yes')",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        // Rename title → heading, remove draft column.
        let new_schema = base_schema("docs", vec![text_column("heading"), text_column("body")]);
        db.alter_collection("docs", &alteration(new_schema, vec![("title", "heading")]))
            .unwrap();

        let conn = db.read_conn().unwrap();
        let (heading, body): (String, String) = conn
            .query_row("SELECT heading, body FROM docs WHERE id = 'd1'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(heading, "Doc");
        assert_eq!(body, "Content");

        // draft column gone.
        let err = conn.query_row("SELECT draft FROM docs WHERE id = 'd1'", [], |r| {
            r.get::<_, String>(0)
        });
        assert!(err.is_err());
    }

    #[test]
    fn alter_rename_nonexistent_source_field_fails() {
        let db = setup_db();
        let schema = base_schema("t", vec![text_column("a")]);
        db.create_collection(&schema).unwrap();

        let new_schema = base_schema("t", vec![text_column("b")]);
        let result = db.alter_collection("t", &alteration(new_schema, vec![("nonexistent", "b")]));
        assert!(result.is_err());
        match result.unwrap_err() {
            DbError::Schema { message } => {
                assert!(message.contains("nonexistent"));
                assert!(message.contains("does not exist"));
            }
            other => panic!("expected Schema error, got: {other:?}"),
        }
    }

    #[test]
    fn alter_rename_target_not_in_schema_fails() {
        let db = setup_db();
        let schema = base_schema("t", vec![text_column("a")]);
        db.create_collection(&schema).unwrap();

        let new_schema = base_schema("t", vec![text_column("c")]);
        let result =
            db.alter_collection("t", &alteration(new_schema, vec![("a", "missing_target")]));
        assert!(result.is_err());
        match result.unwrap_err() {
            DbError::Schema { message } => {
                assert!(message.contains("missing_target"));
                assert!(message.contains("not found in the new schema"));
            }
            other => panic!("expected Schema error, got: {other:?}"),
        }
    }

    #[test]
    fn alter_nonexistent_collection_fails() {
        let db = setup_db();
        let new_schema = base_schema("ghost", vec![text_column("x")]);
        let result = db.alter_collection("ghost", &alteration(new_schema, vec![]));
        assert!(result.is_err());
        match result.unwrap_err() {
            DbError::NotFound { .. } => {}
            other => panic!("expected NotFound, got: {other:?}"),
        }
    }

    #[test]
    fn alter_rename_collection_and_field() {
        let db = setup_db();
        let schema = base_schema("old_tbl", vec![text_column("old_col")]);
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO old_tbl (id, old_col) VALUES ('r1', 'data')",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let new_schema = base_schema("new_tbl", vec![text_column("new_col")]);
        db.alter_collection(
            "old_tbl",
            &alteration(new_schema, vec![("old_col", "new_col")]),
        )
        .unwrap();

        assert!(!db.collection_exists("old_tbl").unwrap());
        assert!(db.collection_exists("new_tbl").unwrap());

        let conn = db.read_conn().unwrap();
        let val: String = conn
            .query_row("SELECT new_col FROM new_tbl WHERE id = 'r1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(val, "data");
    }

    // ── alter_collection — type changes with data migration ───────────

    #[test]
    fn alter_change_type_text_to_real() {
        let db = setup_db();
        let schema = base_schema("products", vec![text_column("price")]);
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO products (id, price) VALUES ('p1', '19.99')",
                [],
            )
            .map_err(DbError::Query)?;
            conn.execute(
                "INSERT INTO products (id, price) VALUES ('p2', 'not-a-number')",
                [],
            )
            .map_err(DbError::Query)?;
            conn.execute("INSERT INTO products (id, price) VALUES ('p3', '42')", [])
                .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        // Change price from TEXT to REAL.
        let new_schema = base_schema("products", vec![real_column("price")]);
        db.alter_collection("products", &alteration(new_schema, vec![]))
            .unwrap();

        let conn = db.read_conn().unwrap();

        // Numeric text should be converted.
        let p1: f64 = conn
            .query_row("SELECT price FROM products WHERE id = 'p1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!((p1 - 19.99).abs() < 0.001);

        // Non-numeric text should become NULL.
        let p2: Option<f64> = conn
            .query_row("SELECT price FROM products WHERE id = 'p2'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(p2.is_none());

        // Integer text should convert fine.
        let p3: f64 = conn
            .query_row("SELECT price FROM products WHERE id = 'p3'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!((p3 - 42.0).abs() < 0.001);
    }

    #[test]
    fn alter_change_type_text_to_integer() {
        let db = setup_db();
        let schema = base_schema("counts", vec![text_column("value")]);
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute("INSERT INTO counts (id, value) VALUES ('c1', '100')", [])
                .map_err(DbError::Query)?;
            conn.execute("INSERT INTO counts (id, value) VALUES ('c2', 'abc')", [])
                .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let new_schema = base_schema("counts", vec![integer_column("value")]);
        db.alter_collection("counts", &alteration(new_schema, vec![]))
            .unwrap();

        let conn = db.read_conn().unwrap();

        let c1: i64 = conn
            .query_row("SELECT value FROM counts WHERE id = 'c1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(c1, 100);

        let c2: Option<i64> = conn
            .query_row("SELECT value FROM counts WHERE id = 'c2'", [], |r| r.get(0))
            .unwrap();
        assert!(c2.is_none());
    }

    #[test]
    fn alter_change_type_real_to_text() {
        let db = setup_db();
        let schema = base_schema("metrics", vec![real_column("score")]);
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute("INSERT INTO metrics (id, score) VALUES ('m1', 3.14)", [])
                .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let new_schema = base_schema("metrics", vec![text_column("score")]);
        db.alter_collection("metrics", &alteration(new_schema, vec![]))
            .unwrap();

        let conn = db.read_conn().unwrap();
        let val: String = conn
            .query_row("SELECT score FROM metrics WHERE id = 'm1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(val.contains("3.14"));
    }

    #[test]
    fn alter_change_type_integer_to_text() {
        let db = setup_db();
        let schema = base_schema("flags", vec![integer_column("active")]);
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute("INSERT INTO flags (id, active) VALUES ('f1', 1)", [])
                .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let new_schema = base_schema("flags", vec![text_column("active")]);
        db.alter_collection("flags", &alteration(new_schema, vec![]))
            .unwrap();

        let conn = db.read_conn().unwrap();
        let val: String = conn
            .query_row("SELECT active FROM flags WHERE id = 'f1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(val, "1");
    }

    #[test]
    fn alter_change_type_integer_to_real() {
        let db = setup_db();
        let schema = base_schema("data", vec![integer_column("val")]);
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute("INSERT INTO data (id, val) VALUES ('d1', 42)", [])
                .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let new_schema = base_schema("data", vec![real_column("val")]);
        db.alter_collection("data", &alteration(new_schema, vec![]))
            .unwrap();

        let conn = db.read_conn().unwrap();
        let val: f64 = conn
            .query_row("SELECT val FROM data WHERE id = 'd1'", [], |r| r.get(0))
            .unwrap();
        assert!((val - 42.0).abs() < 0.001);
    }

    #[test]
    fn alter_rename_and_change_type_simultaneously() {
        let db = setup_db();
        let schema = base_schema("records", vec![text_column("count_str")]);
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO records (id, count_str) VALUES ('r1', '55')",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        // Rename count_str → count and change TEXT → INTEGER.
        let new_schema = base_schema("records", vec![integer_column("count")]);
        db.alter_collection(
            "records",
            &alteration(new_schema, vec![("count_str", "count")]),
        )
        .unwrap();

        let conn = db.read_conn().unwrap();
        let val: i64 = conn
            .query_row("SELECT count FROM records WHERE id = 'r1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(val, 55);
    }

    // ── alter_collection — constraint changes ─────────────────────────

    #[test]
    fn alter_add_not_null_constraint() {
        let db = setup_db();
        let schema = base_schema("items", vec![text_column("name")]);
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute("INSERT INTO items (id, name) VALUES ('i1', 'Widget')", [])
                .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let new_schema = base_schema("items", vec![required_text_column("name")]);
        db.alter_collection("items", &alteration(new_schema, vec![]))
            .unwrap();

        // Existing data preserved.
        let conn = db.read_conn().unwrap();
        let val: String = conn
            .query_row("SELECT name FROM items WHERE id = 'i1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(val, "Widget");

        // NOT NULL constraint now enforced.
        let cols = table_columns(&db, "items");
        let name_col = cols.iter().find(|(n, _, _)| n == "name").unwrap();
        assert!(name_col.2, "name should be NOT NULL");
    }

    #[test]
    fn alter_remove_not_null_constraint() {
        let db = setup_db();
        let schema = base_schema("items", vec![required_text_column("name")]);
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute("INSERT INTO items (id, name) VALUES ('i1', 'Widget')", [])
                .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let new_schema = base_schema("items", vec![text_column("name")]);
        db.alter_collection("items", &alteration(new_schema, vec![]))
            .unwrap();

        let cols = table_columns(&db, "items");
        let name_col = cols.iter().find(|(n, _, _)| n == "name").unwrap();
        assert!(!name_col.2, "name should be nullable now");

        // Can insert NULL.
        db.with_write_conn(|conn| {
            conn.execute("INSERT INTO items (id) VALUES ('i2')", [])
                .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn alter_add_unique_constraint() {
        let db = setup_db();
        let schema = base_schema("slugs", vec![text_column("slug")]);
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute("INSERT INTO slugs (id, slug) VALUES ('s1', 'hello')", [])
                .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let new_schema = base_schema(
            "slugs",
            vec![ColumnDef {
                name: "slug".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: false,
                default: None,
                unique: true,
            }],
        );
        db.alter_collection("slugs", &alteration(new_schema, vec![]))
            .unwrap();

        // Existing data preserved.
        let conn = db.read_conn().unwrap();
        let val: String = conn
            .query_row("SELECT slug FROM slugs WHERE id = 's1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(val, "hello");

        // Duplicate should fail.
        let result = db.with_write_conn(|conn| {
            conn.execute("INSERT INTO slugs (id, slug) VALUES ('s2', 'hello')", [])
                .map_err(DbError::Query)?;
            Ok(())
        });
        assert!(result.is_err());
    }

    #[test]
    fn alter_add_default_value() {
        let db = setup_db();
        let schema = base_schema("config", vec![text_column("value")]);
        db.create_collection(&schema).unwrap();

        let new_schema = base_schema(
            "config",
            vec![ColumnDef {
                name: "value".to_string(),
                sql_type: "TEXT".to_string(),
                not_null: true,
                default: Some("'default_val'".to_string()),
                unique: false,
            }],
        );
        db.alter_collection("config", &alteration(new_schema, vec![]))
            .unwrap();

        // Insert without specifying value — should get the default.
        db.with_write_conn(|conn| {
            conn.execute("INSERT INTO config (id) VALUES ('c1')", [])
                .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let conn = db.read_conn().unwrap();
        let val: String = conn
            .query_row("SELECT value FROM config WHERE id = 'c1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(val, "default_val");
    }

    // ── alter_collection — auth collections ───────────────────────────

    #[test]
    fn alter_auth_collection_preserves_system_columns() {
        let db = setup_db();
        let schema = auth_schema("users", vec![text_column("display_name")]);
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO users (id, email, password, tokenKey, display_name) \
                 VALUES ('u1', 'alice@test.com', 'hash123', 'tk1', 'Alice')",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        // Add a bio field.
        let new_schema = auth_schema(
            "users",
            vec![text_column("display_name"), text_column("bio")],
        );
        db.alter_collection("users", &alteration(new_schema, vec![]))
            .unwrap();

        let conn = db.read_conn().unwrap();
        let (email, password, token_key, display_name): (String, String, String, String) = conn
            .query_row(
                "SELECT email, password, tokenKey, display_name FROM users WHERE id = 'u1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(email, "alice@test.com");
        assert_eq!(password, "hash123");
        assert_eq!(token_key, "tk1");
        assert_eq!(display_name, "Alice");
    }

    #[test]
    fn alter_auth_collection_rename_user_field() {
        let db = setup_db();
        let schema = auth_schema("users", vec![text_column("name")]);
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO users (id, email, password, tokenKey, name) \
                 VALUES ('u1', 'bob@test.com', 'pwd', 'tk', 'Bob')",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        // Rename name → full_name in an auth collection.
        let new_schema = auth_schema("users", vec![text_column("full_name")]);
        db.alter_collection(
            "users",
            &alteration(new_schema, vec![("name", "full_name")]),
        )
        .unwrap();

        let conn = db.read_conn().unwrap();
        let (email, full_name): (String, String) = conn
            .query_row(
                "SELECT email, full_name FROM users WHERE id = 'u1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(email, "bob@test.com");
        assert_eq!(full_name, "Bob");
    }

    #[test]
    fn alter_auth_collection_preserves_email_index() {
        let db = setup_db();
        let schema = auth_schema("users", vec![text_column("name")]);
        db.create_collection(&schema).unwrap();

        // Add a field via alter.
        let new_schema = auth_schema("users", vec![text_column("name"), text_column("avatar")]);
        db.alter_collection("users", &alteration(new_schema, vec![]))
            .unwrap();

        // Verify email unique index still works.
        db.with_write_conn(|conn| {
            conn.execute("INSERT INTO users (id, email) VALUES ('u1', 'a@b.com')", [])
                .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let result = db.with_write_conn(|conn| {
            conn.execute("INSERT INTO users (id, email) VALUES ('u2', 'a@b.com')", [])
                .map_err(DbError::Query)?;
            Ok(())
        });
        assert!(result.is_err(), "duplicate email should fail");
    }

    // ── alter_collection — data preservation edge cases ────────────────

    #[test]
    fn alter_preserves_multiple_rows() {
        let db = setup_db();
        let schema = base_schema("items", vec![text_column("name"), real_column("price")]);
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            for i in 0..100 {
                conn.execute(
                    "INSERT INTO items (id, name, price) VALUES (?1, ?2, ?3)",
                    params![format!("i{}", i), format!("Item {}", i), i as f64 * 1.5],
                )
                .map_err(DbError::Query)?;
            }
            Ok(())
        })
        .unwrap();

        // Add a column and rename another.
        let new_schema = base_schema(
            "items",
            vec![
                text_column("label"),
                real_column("price"),
                text_column("category"),
            ],
        );
        db.alter_collection("items", &alteration(new_schema, vec![("name", "label")]))
            .unwrap();

        let conn = db.read_conn().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 100);

        // Spot-check a few rows.
        let (label, price): (String, f64) = conn
            .query_row("SELECT label, price FROM items WHERE id = 'i42'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(label, "Item 42");
        assert!((price - 63.0).abs() < 0.001);
    }

    #[test]
    fn alter_preserves_null_values() {
        let db = setup_db();
        let schema = base_schema("data", vec![text_column("a"), text_column("b")]);
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute("INSERT INTO data (id, a) VALUES ('d1', 'has_a')", [])
                .map_err(DbError::Query)?;
            conn.execute("INSERT INTO data (id, b) VALUES ('d2', 'has_b')", [])
                .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        // Rename a → alpha.
        let new_schema = base_schema("data", vec![text_column("alpha"), text_column("b")]);
        db.alter_collection("data", &alteration(new_schema, vec![("a", "alpha")]))
            .unwrap();

        let conn = db.read_conn().unwrap();
        let (alpha, b): (Option<String>, Option<String>) = conn
            .query_row("SELECT alpha, b FROM data WHERE id = 'd1'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(alpha.as_deref(), Some("has_a"));
        assert!(b.is_none());
    }

    #[test]
    fn alter_preserves_timestamps() {
        let db = setup_db();
        let schema = base_schema("events", vec![text_column("name")]);
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO events (id, name, created, updated) \
                 VALUES ('e1', 'event', '2024-01-15 10:00:00', '2024-06-20 14:30:00')",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        // Add a field.
        let new_schema = base_schema("events", vec![text_column("name"), text_column("location")]);
        db.alter_collection("events", &alteration(new_schema, vec![]))
            .unwrap();

        let conn = db.read_conn().unwrap();
        let (created, updated): (String, String) = conn
            .query_row(
                "SELECT created, updated FROM events WHERE id = 'e1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(created, "2024-01-15 10:00:00");
        assert_eq!(updated, "2024-06-20 14:30:00");
    }

    // ── alter_collection — indexes ────────────────────────────────────

    #[test]
    fn alter_preserves_user_defined_indexes() {
        let db = setup_db();
        let schema = CollectionSchema {
            name: "articles".to_string(),
            collection_type: "base".to_string(),
            columns: vec![text_column("title"), text_column("slug")],
            indexes: vec![IndexDef {
                name: "idx_articles_slug".to_string(),
                columns: vec!["slug".to_string()],
                index_columns: vec![],
                unique: true,
            }],
            searchable_fields: vec![],
            view_query: None,
        };
        db.create_collection(&schema).unwrap();

        // Add a column, keep the index.
        let new_schema = CollectionSchema {
            name: "articles".to_string(),
            collection_type: "base".to_string(),
            columns: vec![
                text_column("title"),
                text_column("slug"),
                text_column("body"),
            ],
            indexes: vec![IndexDef {
                name: "idx_articles_slug".to_string(),
                columns: vec!["slug".to_string()],
                index_columns: vec![],
                unique: true,
            }],
            searchable_fields: vec![],
            view_query: None,
        };
        db.alter_collection("articles", &alteration(new_schema, vec![]))
            .unwrap();

        let idxs = table_indexes(&db, "articles");
        assert!(
            idxs.iter().any(|(name, _)| name == "idx_articles_slug"),
            "user index should be preserved"
        );
    }

    #[test]
    fn alter_adds_new_index() {
        let db = setup_db();
        let schema = base_schema("posts", vec![text_column("title"), text_column("slug")]);
        db.create_collection(&schema).unwrap();

        let new_schema = CollectionSchema {
            name: "posts".to_string(),
            collection_type: "base".to_string(),
            columns: vec![text_column("title"), text_column("slug")],
            indexes: vec![IndexDef {
                name: "idx_posts_slug".to_string(),
                columns: vec!["slug".to_string()],
                index_columns: vec![],
                unique: true,
            }],
            searchable_fields: vec![],
            view_query: None,
        };
        db.alter_collection("posts", &alteration(new_schema, vec![]))
            .unwrap();

        let idxs = table_indexes(&db, "posts");
        assert!(idxs.iter().any(|(name, _)| name == "idx_posts_slug"));
    }

    // ── alter_collection — no-op and empty alterations ────────────────

    #[test]
    fn alter_with_no_changes_preserves_data() {
        let db = setup_db();
        let schema = base_schema("stable", vec![text_column("name")]);
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO stable (id, name) VALUES ('s1', 'unchanged')",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        // Same schema, no renames.
        let same_schema = base_schema("stable", vec![text_column("name")]);
        db.alter_collection("stable", &alteration(same_schema, vec![]))
            .unwrap();

        let conn = db.read_conn().unwrap();
        let val: String = conn
            .query_row("SELECT name FROM stable WHERE id = 's1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(val, "unchanged");
    }

    #[test]
    fn alter_add_field_only() {
        let db = setup_db();
        let schema = base_schema("simple", vec![text_column("a")]);
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute("INSERT INTO simple (id, a) VALUES ('s1', 'val_a')", [])
                .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let new_schema = base_schema("simple", vec![text_column("a"), text_column("b")]);
        db.alter_collection("simple", &alteration(new_schema, vec![]))
            .unwrap();

        let conn = db.read_conn().unwrap();
        let (a, b): (String, Option<String>) = conn
            .query_row("SELECT a, b FROM simple WHERE id = 's1'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(a, "val_a");
        assert!(b.is_none());
    }

    #[test]
    fn alter_remove_field_only() {
        let db = setup_db();
        let schema = base_schema("dropme", vec![text_column("keep"), text_column("remove")]);
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO dropme (id, keep, remove) VALUES ('d1', 'kept', 'gone')",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        let new_schema = base_schema("dropme", vec![text_column("keep")]);
        db.alter_collection("dropme", &alteration(new_schema, vec![]))
            .unwrap();

        let conn = db.read_conn().unwrap();
        let val: String = conn
            .query_row("SELECT keep FROM dropme WHERE id = 'd1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(val, "kept");

        let err = conn.query_row("SELECT remove FROM dropme WHERE id = 'd1'", [], |r| {
            r.get::<_, String>(0)
        });
        assert!(err.is_err());
    }

    // ── alter_collection — complex combined scenarios ──────────────────

    #[test]
    fn alter_complex_multi_operation() {
        let db = setup_db();
        let schema = base_schema(
            "products",
            vec![
                text_column("name"),
                text_column("price_str"),
                text_column("obsolete"),
                integer_column("stock"),
            ],
        );
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO products (id, name, price_str, obsolete, stock) \
                 VALUES ('p1', 'Widget', '29.99', 'old_data', 50)",
                [],
            )
            .map_err(DbError::Query)?;
            conn.execute(
                "INSERT INTO products (id, name, price_str, obsolete, stock) \
                 VALUES ('p2', 'Gadget', 'invalid', 'old_data', 0)",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        // Combined operation:
        // - Rename name → title
        // - Rename price_str → price AND change type TEXT → REAL
        // - Remove obsolete
        // - Keep stock
        // - Add new category column
        let new_schema = base_schema(
            "products",
            vec![
                text_column("title"),
                real_column("price"),
                integer_column("stock"),
                text_column("category"),
            ],
        );
        db.alter_collection(
            "products",
            &alteration(new_schema, vec![("name", "title"), ("price_str", "price")]),
        )
        .unwrap();

        let conn = db.read_conn().unwrap();

        // Row 1: numeric price converts.
        let (title, price, stock, category): (String, f64, i64, Option<String>) = conn
            .query_row(
                "SELECT title, price, stock, category FROM products WHERE id = 'p1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(title, "Widget");
        assert!((price - 29.99).abs() < 0.001);
        assert_eq!(stock, 50);
        assert!(category.is_none());

        // Row 2: invalid price becomes NULL.
        let price2: Option<f64> = conn
            .query_row("SELECT price FROM products WHERE id = 'p2'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(price2.is_none());

        // Obsolete column gone.
        let err = conn.query_row("SELECT obsolete FROM products WHERE id = 'p1'", [], |r| {
            r.get::<_, String>(0)
        });
        assert!(err.is_err());

        // Metadata correct.
        let retrieved = db.get_collection("products").unwrap();
        assert_eq!(retrieved.columns.len(), 4);
        let col_names: Vec<&str> = retrieved.columns.iter().map(|c| c.name.as_str()).collect();
        assert!(col_names.contains(&"title"));
        assert!(col_names.contains(&"price"));
        assert!(col_names.contains(&"stock"));
        assert!(col_names.contains(&"category"));
        assert!(!col_names.contains(&"name"));
        assert!(!col_names.contains(&"price_str"));
        assert!(!col_names.contains(&"obsolete"));
    }

    #[test]
    fn alter_collection_rename_to_existing_name_conflicts() {
        let db = setup_db();
        db.create_collection(&base_schema("alpha", vec![text_column("a")]))
            .unwrap();
        db.create_collection(&base_schema("beta", vec![text_column("b")]))
            .unwrap();

        let new_schema = base_schema("beta", vec![text_column("a")]);
        let result = db.alter_collection("alpha", &alteration(new_schema, vec![]));
        assert!(result.is_err());
        match result.unwrap_err() {
            DbError::Conflict { .. } => {}
            other => panic!("expected Conflict, got: {other:?}"),
        }
    }

    #[test]
    fn alter_reorder_columns() {
        let db = setup_db();
        let schema = base_schema(
            "ordered",
            vec![
                text_column("first"),
                text_column("second"),
                text_column("third"),
            ],
        );
        db.create_collection(&schema).unwrap();

        db.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO ordered (id, first, second, third) VALUES ('o1', 'A', 'B', 'C')",
                [],
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        // Reorder columns (reverse).
        let new_schema = base_schema(
            "ordered",
            vec![
                text_column("third"),
                text_column("second"),
                text_column("first"),
            ],
        );
        db.alter_collection("ordered", &alteration(new_schema, vec![]))
            .unwrap();

        let conn = db.read_conn().unwrap();
        let (first, second, third): (String, String, String) = conn
            .query_row(
                "SELECT first, second, third FROM ordered WHERE id = 'o1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(first, "A");
        assert_eq!(second, "B");
        assert_eq!(third, "C");

        // Metadata reflects new order.
        let retrieved = db.get_collection("ordered").unwrap();
        assert_eq!(retrieved.columns[0].name, "third");
        assert_eq!(retrieved.columns[1].name, "second");
        assert_eq!(retrieved.columns[2].name, "first");
    }

    // ── Index sort direction tests ──────────────────────────────────────────

    #[test]
    fn create_index_with_desc_sort_direction() {
        let db = setup_db();
        let schema = CollectionSchema {
            name: "events".to_string(),
            collection_type: "base".to_string(),
            columns: vec![text_column("title"), text_column("date")],
            indexes: vec![IndexDef {
                name: "idx_events_date_desc".to_string(),
                columns: vec!["date".to_string()],
                index_columns: vec![IndexColumnDef {
                    name: "date".to_string(),
                    sort: IndexColumnSort::Desc,
                }],
                unique: false,
            }],
            searchable_fields: vec![],
            view_query: None,
        };
        db.create_collection(&schema).unwrap();

        // Verify the index was created by loading it back.
        let loaded = db.get_collection("events").unwrap();
        let idx = loaded
            .indexes
            .iter()
            .find(|i| i.name == "idx_events_date_desc")
            .expect("desc index should exist");
        assert_eq!(idx.index_columns.len(), 1);
        assert_eq!(idx.index_columns[0].sort, IndexColumnSort::Desc);
    }

    #[test]
    fn create_composite_index_with_mixed_sort_directions() {
        let db = setup_db();
        let schema = CollectionSchema {
            name: "logs".to_string(),
            collection_type: "base".to_string(),
            columns: vec![text_column("category"), text_column("timestamp")],
            indexes: vec![IndexDef {
                name: "idx_logs_cat_asc_ts_desc".to_string(),
                columns: vec!["category".to_string(), "timestamp".to_string()],
                index_columns: vec![
                    IndexColumnDef {
                        name: "category".to_string(),
                        sort: IndexColumnSort::Asc,
                    },
                    IndexColumnDef {
                        name: "timestamp".to_string(),
                        sort: IndexColumnSort::Desc,
                    },
                ],
                unique: false,
            }],
            searchable_fields: vec![],
            view_query: None,
        };
        db.create_collection(&schema).unwrap();

        let loaded = db.get_collection("logs").unwrap();
        let idx = loaded
            .indexes
            .iter()
            .find(|i| i.name == "idx_logs_cat_asc_ts_desc")
            .expect("composite index should exist");
        assert_eq!(idx.index_columns.len(), 2);
        assert_eq!(idx.index_columns[0].name, "category");
        assert_eq!(idx.index_columns[0].sort, IndexColumnSort::Asc);
        assert_eq!(idx.index_columns[1].name, "timestamp");
        assert_eq!(idx.index_columns[1].sort, IndexColumnSort::Desc);
    }

    #[test]
    fn index_survives_table_rebuild() {
        let db = setup_db();
        let schema = CollectionSchema {
            name: "articles".to_string(),
            collection_type: "base".to_string(),
            columns: vec![text_column("title"), text_column("slug")],
            indexes: vec![IndexDef {
                name: "idx_articles_slug".to_string(),
                columns: vec!["slug".to_string()],
                index_columns: vec![],
                unique: true,
            }],
            searchable_fields: vec![],
            view_query: None,
        };
        db.create_collection(&schema).unwrap();

        // Update schema (adds a column, triggers rebuild).
        let updated = CollectionSchema {
            name: "articles".to_string(),
            collection_type: "base".to_string(),
            columns: vec![
                text_column("title"),
                text_column("slug"),
                text_column("body"),
            ],
            indexes: vec![IndexDef {
                name: "idx_articles_slug".to_string(),
                columns: vec!["slug".to_string()],
                index_columns: vec![],
                unique: true,
            }],
            searchable_fields: vec![],
            view_query: None,
        };
        db.update_collection("articles", &updated).unwrap();

        // Index should still be present after rebuild.
        let loaded = db.get_collection("articles").unwrap();
        assert!(loaded
            .indexes
            .iter()
            .any(|i| i.name == "idx_articles_slug" && i.unique));
    }

    #[test]
    fn index_improves_query_performance() {
        let db = setup_db();
        let schema = CollectionSchema {
            name: "perf_test".to_string(),
            collection_type: "base".to_string(),
            columns: vec![text_column("category"), text_column("value")],
            indexes: vec![],
            searchable_fields: vec![],
            view_query: None,
        };
        db.create_collection(&schema).unwrap();

        // Insert 1000 records using raw SQL.
        db.with_write_conn(|conn| {
            for i in 0..1000 {
                let id = format!("rec_{:04}", i);
                conn.execute(
                    "INSERT INTO \"perf_test\" (id, category, value, created, updated) VALUES (?1, ?2, ?3, datetime('now'), datetime('now'))",
                    params![id, format!("cat_{}", i % 10), format!("val_{}", i)],
                ).map_err(DbError::Query)?;
            }
            Ok(())
        }).unwrap();

        // Verify records were inserted.
        {
            let conn = db.read_conn().unwrap();
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM \"perf_test\"", [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, 1000);
        }

        // Add index on category.
        let indexed_schema = CollectionSchema {
            name: "perf_test".to_string(),
            collection_type: "base".to_string(),
            columns: vec![text_column("category"), text_column("value")],
            indexes: vec![IndexDef {
                name: "idx_perf_test_category".to_string(),
                columns: vec!["category".to_string()],
                index_columns: vec![],
                unique: false,
            }],
            searchable_fields: vec![],
            view_query: None,
        };
        db.update_collection("perf_test", &indexed_schema).unwrap();

        // Verify the index exists and is used via EXPLAIN QUERY PLAN.
        let conn = db.read_conn().unwrap();
        let explain_sql =
            "EXPLAIN QUERY PLAN SELECT * FROM \"perf_test\" WHERE \"category\" = 'cat_5'";
        let plan: String = conn.query_row(explain_sql, [], |row| row.get(3)).unwrap();
        assert!(
            plan.contains("idx_perf_test_category"),
            "query plan should use the index, got: {plan}"
        );

        // Also verify the filtered query returns correct results.
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM \"perf_test\" WHERE \"category\" = 'cat_5'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 100);
    }

    #[test]
    fn drop_index_on_collection_update() {
        let db = setup_db();
        let schema = CollectionSchema {
            name: "indexed".to_string(),
            collection_type: "base".to_string(),
            columns: vec![text_column("a"), text_column("b")],
            indexes: vec![
                IndexDef {
                    name: "idx_indexed_a".to_string(),
                    columns: vec!["a".to_string()],
                    index_columns: vec![],
                    unique: false,
                },
                IndexDef {
                    name: "idx_indexed_b".to_string(),
                    columns: vec!["b".to_string()],
                    index_columns: vec![],
                    unique: false,
                },
            ],
            searchable_fields: vec![],
            view_query: None,
        };
        db.create_collection(&schema).unwrap();

        // Remove one index.
        let updated = CollectionSchema {
            name: "indexed".to_string(),
            collection_type: "base".to_string(),
            columns: vec![text_column("a"), text_column("b")],
            indexes: vec![IndexDef {
                name: "idx_indexed_a".to_string(),
                columns: vec!["a".to_string()],
                index_columns: vec![],
                unique: false,
            }],
            searchable_fields: vec![],
            view_query: None,
        };
        db.update_collection("indexed", &updated).unwrap();

        let loaded = db.get_collection("indexed").unwrap();
        // Should have the one user index plus system indexes (e.g., idx_indexed_created).
        assert!(
            loaded.indexes.iter().any(|i| i.name == "idx_indexed_a"),
            "idx_indexed_a should still exist"
        );
        assert!(
            !loaded.indexes.iter().any(|i| i.name == "idx_indexed_b"),
            "idx_indexed_b should have been removed"
        );
    }

    // ── View collection tests ────────────────────────────────────────────

    /// Helper: create a base "posts" table to use as the source for views.
    fn setup_db_with_posts_for_views() -> Database {
        let db = Database::open_in_memory(&PoolConfig::default()).unwrap();
        db.run_system_migrations().unwrap();

        let schema = CollectionSchema {
            name: "posts".to_string(),
            collection_type: "base".to_string(),
            columns: vec![
                ColumnDef {
                    name: "title".to_string(),
                    sql_type: "TEXT".to_string(),
                    not_null: true,
                    default: None,
                    unique: false,
                },
                ColumnDef {
                    name: "views".to_string(),
                    sql_type: "INTEGER".to_string(),
                    not_null: false,
                    default: Some("0".to_string()),
                    unique: false,
                },
            ],
            indexes: vec![],
            searchable_fields: vec![],
            view_query: None,
        };
        db.create_collection(&schema).unwrap();

        // Insert some seed data.
        db.with_write_conn(|conn| {
            conn.execute_batch(
                "INSERT INTO posts (id, title, views) VALUES ('p1', 'Hello', 10);
                 INSERT INTO posts (id, title, views) VALUES ('p2', 'World', 20);",
            )
            .map_err(DbError::Query)?;
            Ok(())
        })
        .unwrap();

        db
    }

    #[test]
    fn create_view_collection() {
        let db = setup_db_with_posts_for_views();

        let view_schema = CollectionSchema {
            name: "popular_posts".to_string(),
            collection_type: "view".to_string(),
            columns: vec![],
            indexes: vec![],
            searchable_fields: vec![],
            view_query: Some(
                "SELECT id, title, views, created, updated FROM posts WHERE views > 5".to_string(),
            ),
        };
        db.create_collection(&view_schema).unwrap();

        // Verify view exists in _collections.
        assert!(db.collection_exists("popular_posts").unwrap());

        // Can read the view.
        let loaded = db.get_collection("popular_posts").unwrap();
        assert_eq!(loaded.collection_type, "view");
        assert_eq!(
            loaded.view_query.as_deref(),
            Some("SELECT id, title, views, created, updated FROM posts WHERE views > 5")
        );
    }

    #[test]
    fn view_collection_infers_columns() {
        let db = setup_db_with_posts_for_views();

        let view_schema = CollectionSchema {
            name: "post_titles".to_string(),
            collection_type: "view".to_string(),
            columns: vec![],
            indexes: vec![],
            searchable_fields: vec![],
            view_query: Some("SELECT id, title, created, updated FROM posts".to_string()),
        };
        db.create_collection(&view_schema).unwrap();

        let loaded = db.get_collection("post_titles").unwrap();
        // System columns (id, created, updated) are filtered out.
        assert_eq!(loaded.columns.len(), 1);
        assert_eq!(loaded.columns[0].name, "title");
    }

    #[test]
    fn view_collection_data_is_queryable() {
        let db = setup_db_with_posts_for_views();

        let view_schema = CollectionSchema {
            name: "all_posts_view".to_string(),
            collection_type: "view".to_string(),
            columns: vec![],
            indexes: vec![],
            searchable_fields: vec![],
            view_query: Some(
                "SELECT id, title, views, created, updated FROM posts".to_string(),
            ),
        };
        db.create_collection(&view_schema).unwrap();

        // Query data through the view using raw SQL (RecordRepository).
        let conn = db.read_conn().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM all_posts_view", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn view_collection_with_filter() {
        let db = setup_db_with_posts_for_views();

        let view_schema = CollectionSchema {
            name: "high_views".to_string(),
            collection_type: "view".to_string(),
            columns: vec![],
            indexes: vec![],
            searchable_fields: vec![],
            view_query: Some(
                "SELECT id, title, views, created, updated FROM posts WHERE views > 15".to_string(),
            ),
        };
        db.create_collection(&view_schema).unwrap();

        let conn = db.read_conn().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM high_views", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1); // Only "World" with views=20.
    }

    #[test]
    fn delete_view_collection() {
        let db = setup_db_with_posts_for_views();

        let view_schema = CollectionSchema {
            name: "temp_view".to_string(),
            collection_type: "view".to_string(),
            columns: vec![],
            indexes: vec![],
            searchable_fields: vec![],
            view_query: Some(
                "SELECT id, title, created, updated FROM posts".to_string(),
            ),
        };
        db.create_collection(&view_schema).unwrap();
        assert!(db.collection_exists("temp_view").unwrap());

        db.delete_collection("temp_view").unwrap();
        assert!(!db.collection_exists("temp_view").unwrap());
    }

    #[test]
    fn view_collection_requires_view_query() {
        let db = Database::open_in_memory(&PoolConfig::default()).unwrap();
        db.run_system_migrations().unwrap();

        let view_schema = CollectionSchema {
            name: "bad_view".to_string(),
            collection_type: "view".to_string(),
            columns: vec![],
            indexes: vec![],
            searchable_fields: vec![],
            view_query: None, // Missing!
        };
        let result = db.create_collection(&view_schema);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("view_query"),
            "error should mention view_query: {err_msg}"
        );
    }

    #[test]
    fn view_collection_validates_sql_syntax() {
        let db = Database::open_in_memory(&PoolConfig::default()).unwrap();
        db.run_system_migrations().unwrap();

        let view_schema = CollectionSchema {
            name: "bad_sql_view".to_string(),
            collection_type: "view".to_string(),
            columns: vec![],
            indexes: vec![],
            searchable_fields: vec![],
            view_query: Some("SELECT * FROM nonexistent_table".to_string()),
        };
        let result = db.create_collection(&view_schema);
        assert!(result.is_err(), "should fail for invalid SQL query");
    }

    #[test]
    fn list_collections_includes_views() {
        let db = setup_db_with_posts_for_views();

        let view_schema = CollectionSchema {
            name: "listed_view".to_string(),
            collection_type: "view".to_string(),
            columns: vec![],
            indexes: vec![],
            searchable_fields: vec![],
            view_query: Some(
                "SELECT id, title, created, updated FROM posts".to_string(),
            ),
        };
        db.create_collection(&view_schema).unwrap();

        let collections = db.list_collections().unwrap();
        let view = collections.iter().find(|c| c.name == "listed_view");
        assert!(view.is_some(), "view should appear in list_collections");
        assert_eq!(view.unwrap().collection_type, "view");
        assert!(view.unwrap().view_query.is_some());
    }
}
