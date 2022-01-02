use anyhow::Context;

use crate::db::Conn;

pub fn set_up_helpers(db: &mut dyn Conn, current_migration: &Option<String>) -> anyhow::Result<()> {
    let predicate = if let Some(current_migration) = current_migration {
        format!(
            "current_setting('search_path') = 'migration_{}' OR setting_bool",
            current_migration
        )
    } else {
        format!("setting_bool")
    };

    let query = format!(
        "
			CREATE OR REPLACE FUNCTION reshape.is_old_schema()
			RETURNS BOOLEAN AS $$
            DECLARE
                setting TEXT := current_setting('reshape.is_old_schema', TRUE);
                setting_bool BOOLEAN := setting IS NOT NULL AND setting = 'YES';
			BEGIN
				RETURN {};
			END
			$$ language 'plpgsql';
        ",
        predicate
    );
    db.query(&query)
        .context("failed creating helper function reshape.is_old_schema()")?;

    Ok(())
}

pub fn tear_down_helpers(db: &mut dyn Conn) -> anyhow::Result<()> {
    db.query("DROP FUNCTION reshape.is_old_schema;")?;
    Ok(())
}
