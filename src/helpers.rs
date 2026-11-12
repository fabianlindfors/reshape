use anyhow::Context;

use crate::db::Conn;

pub fn set_up_helpers(db: &mut dyn Conn, target_migration: &str) -> anyhow::Result<()> {
    // Applications can append schemas such as `public` for extension types. Compare
    // the first effective schema, not the entire search_path, or their writes will
    // incorrectly run the old-schema triggers. An empty path must return false, not NULL.
    let query = format!(
        "
			CREATE OR REPLACE FUNCTION reshape.is_new_schema()
			RETURNS BOOLEAN AS $$
            DECLARE
                setting TEXT := current_setting('reshape.is_new_schema', TRUE);
                setting_bool BOOLEAN := setting IS NOT NULL AND setting = 'YES';
			BEGIN
				RETURN current_schema() IS NOT DISTINCT FROM '{}' OR setting_bool;
			END
			$$ language 'plpgsql';
        ",
        crate::schema_name_for_migration(target_migration),
    );
    db.query(&query)
        .context("failed creating helper function reshape.is_new_schema()")?;

    Ok(())
}

pub fn tear_down_helpers(db: &mut dyn Conn) -> anyhow::Result<()> {
    db.query("DROP FUNCTION IF EXISTS reshape.is_new_schema;")?;
    Ok(())
}
