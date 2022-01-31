use anyhow::Context;

use crate::db::Conn;

pub fn set_up_helpers(db: &mut dyn Conn, target_migration: &str) -> anyhow::Result<()> {
    let query = format!(
        "
			CREATE OR REPLACE FUNCTION reshape.is_new_schema()
			RETURNS BOOLEAN AS $$
            DECLARE
                setting TEXT := current_setting('reshape.is_new_schema', TRUE);
                setting_bool BOOLEAN := setting IS NOT NULL AND setting = 'YES';
			BEGIN
				RETURN current_setting('search_path') = 'migration_{}' OR setting_bool;
			END
			$$ language 'plpgsql';
        ",
        target_migration,
    );
    db.query(&query)
        .context("failed creating helper function reshape.is_new_schema()")?;

    Ok(())
}

pub fn tear_down_helpers(db: &mut dyn Conn) -> anyhow::Result<()> {
    db.query("DROP FUNCTION IF EXISTS reshape.is_new_schema;")?;
    Ok(())
}
