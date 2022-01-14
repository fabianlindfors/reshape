use anyhow::anyhow;
use postgres::{types::ToSql, NoTls, Row};

// DbLocker wraps a regular DbConn, only allowing access using the
// `lock` method. This method will acquire the advisory lock before
// allowing access to the database, and then release it afterwards.
//
// We use advisory locks to avoid multiple Reshape instances working
// on the same database as the same time. DbLocker is the only way to
// get a DbConn which ensures that all database access is protected by
// a lock.
//
// Postgres docs on advisory locks:
//   https://www.postgresql.org/docs/current/explicit-locking.html#ADVISORY-LOCKS
pub struct DbLocker {
    client: DbConn,
}

impl DbLocker {
    // Advisory lock keys in Postgres are 64-bit integers.
    // The key we use was chosen randomly.
    const LOCK_KEY: i64 = 4036779288569897133;

    pub fn connect(config: &postgres::Config) -> anyhow::Result<Self> {
        let pg = config.connect(NoTls)?;
        Ok(Self {
            client: DbConn::new(pg),
        })
    }

    pub fn lock(
        &mut self,
        f: impl FnOnce(&mut DbConn) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        self.acquire_lock()?;
        let result = f(&mut self.client);
        self.release_lock()?;

        result
    }

    fn acquire_lock(&mut self) -> anyhow::Result<()> {
        let success = self
            .client
            .query(&format!("SELECT pg_try_advisory_lock({})", Self::LOCK_KEY))?
            .first()
            .ok_or_else(|| anyhow!("unexpectedly failed when acquiring advisory lock"))
            .map(|row| row.get::<'_, _, bool>(0))?;

        if success {
            Ok(())
        } else {
            Err(anyhow!("another instance of Reshape is already running"))
        }
    }

    fn release_lock(&mut self) -> anyhow::Result<()> {
        self.client
            .query(&format!("SELECT pg_advisory_unlock({})", Self::LOCK_KEY))?
            .first()
            .ok_or_else(|| anyhow!("unexpectedly failed when releasing advisory lock"))?;
        Ok(())
    }
}

pub trait Conn {
    fn run(&mut self, query: &str) -> anyhow::Result<()>;
    fn query(&mut self, query: &str) -> anyhow::Result<Vec<Row>>;
    fn query_with_params(
        &mut self,
        query: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> anyhow::Result<Vec<Row>>;
    fn transaction(&mut self) -> anyhow::Result<Transaction>;
}

pub struct DbConn {
    client: postgres::Client,
}

impl DbConn {
    fn new(client: postgres::Client) -> Self {
        DbConn { client }
    }
}

impl Conn for DbConn {
    fn run(&mut self, query: &str) -> anyhow::Result<()> {
        self.client.batch_execute(query)?;
        Ok(())
    }

    fn query(&mut self, query: &str) -> anyhow::Result<Vec<Row>> {
        let rows = self.client.query(query, &[])?;
        Ok(rows)
    }

    fn query_with_params(
        &mut self,
        query: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> anyhow::Result<Vec<Row>> {
        let rows = self.client.query(query, params)?;
        Ok(rows)
    }

    fn transaction(&mut self) -> anyhow::Result<Transaction> {
        let transaction = self.client.transaction()?;
        Ok(Transaction { transaction })
    }
}

pub struct Transaction<'a> {
    transaction: postgres::Transaction<'a>,
}

impl Transaction<'_> {
    pub fn commit(self) -> anyhow::Result<()> {
        self.transaction.commit()?;
        Ok(())
    }

    pub fn rollback(self) -> anyhow::Result<()> {
        self.transaction.rollback()?;
        Ok(())
    }
}

impl Conn for Transaction<'_> {
    fn run(&mut self, query: &str) -> anyhow::Result<()> {
        self.transaction.batch_execute(query)?;
        Ok(())
    }

    fn query(&mut self, query: &str) -> anyhow::Result<Vec<Row>> {
        let rows = self.transaction.query(query, &[])?;
        Ok(rows)
    }

    fn query_with_params(
        &mut self,
        query: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> anyhow::Result<Vec<Row>> {
        let rows = self.transaction.query(query, params)?;
        Ok(rows)
    }

    fn transaction(&mut self) -> anyhow::Result<Transaction> {
        let transaction = self.transaction.transaction()?;
        Ok(Transaction { transaction })
    }
}
