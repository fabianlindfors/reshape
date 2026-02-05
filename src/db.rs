use std::{cmp::min, time::Duration};

use anyhow::{anyhow, Context};
use postgres::{types::ToSql, NoTls, Row};
use rand::prelude::*;

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
        let mut pg = config.connect(NoTls)?;

        // When running DDL queries that acquire locks, we risk causing a "lock queue".
        // When attempting to acquire a lock, Postgres will wait for any long running queries to complete.
        // At the same time, it will block other queries until the lock has been acquired and released.
        // This has the bad effect of the long-running query blocking other queries because of us, forming
        // a queue of other queries until we release our lock.
        //
        // We set the lock_timeout setting to avoid this. This puts an upper bound for how long Postgres will
        // wait to acquire locks and also the maximum amount of time a long-running query can block other queries.
        // We should also add automatic retries to handle these timeouts gracefully.
        //
        // Reference: https://medium.com/paypal-tech/postgresql-at-scale-database-schema-changes-without-downtime-20d3749ed680
        //
        // TODO: Make lock_timeout configurable
        pg.simple_query("SET lock_timeout = '1s'")
            .context("failed to set lock_timeout")?;

        Ok(Self {
            client: DbConn::new(pg),
        })
    }

    pub fn lock<T>(
        &mut self,
        f: impl FnOnce(&mut DbConn) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
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
    fn transaction(&mut self) -> anyhow::Result<Transaction<'_>>;
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
        retry_automatically(|| self.client.batch_execute(query))?;
        Ok(())
    }

    fn query(&mut self, query: &str) -> anyhow::Result<Vec<Row>> {
        let rows = retry_automatically(|| self.client.query(query, &[]))?;
        Ok(rows)
    }

    fn query_with_params(
        &mut self,
        query: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> anyhow::Result<Vec<Row>> {
        let rows = retry_automatically(|| self.client.query(query, params))?;
        Ok(rows)
    }

    fn transaction(&mut self) -> anyhow::Result<Transaction<'_>> {
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

    fn transaction(&mut self) -> anyhow::Result<Transaction<'_>> {
        let transaction = self.transaction.transaction()?;
        Ok(Transaction { transaction })
    }
}

// Retry a database operation with exponential backoff and jitter
fn retry_automatically<T>(
    mut f: impl FnMut() -> Result<T, postgres::Error>,
) -> Result<T, postgres::Error> {
    const STARTING_WAIT_TIME: u64 = 100;
    const MAX_WAIT_TIME: u64 = 3_200;
    const MAX_ATTEMPTS: u32 = 10;

    let mut rng = rand::rng();
    let mut attempts = 0;
    loop {
        let result = f();

        let error = match result {
            Ok(_) => return result,
            Err(err) => err,
        };

        // If we got a database error, we check if it's retryable.
        // If we didn't get a database error, then it's most likely some kind of connection
        // error which should also be retried.
        if let Some(db_error) = error.as_db_error() {
            if !error_retryable(db_error) {
                return Err(error);
            }
        }

        attempts += 1;
        if attempts >= MAX_ATTEMPTS {
            return Err(error);
        }

        // The wait time increases exponentially, starting at 100ms and doubling up to a max of 3.2s.
        let wait_time = min(
            MAX_WAIT_TIME,
            STARTING_WAIT_TIME * u64::pow(2, attempts - 1),
        );

        // The jitter is up to half the wait time
        let jitter: u64 = rng.random_range(0..wait_time / 2);

        std::thread::sleep(Duration::from_millis(wait_time + jitter));
    }
}

// Check if a database error can be retried
fn error_retryable(error: &postgres::error::DbError) -> bool {
    // LOCK_NOT_AVAILABLE is caused by lock_timeout being exceeded
    matches!(error.code(), &postgres::error::SqlState::LOCK_NOT_AVAILABLE)
}
