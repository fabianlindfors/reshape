use postgres::{types::ToSql, NoTls, Row};

pub trait Conn {
    fn run(&mut self, query: &str) -> anyhow::Result<()>;
    fn query(&mut self, query: &str) -> anyhow::Result<Vec<Row>>;
    fn query_with_params(
        &mut self,
        query: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> anyhow::Result<Vec<Row>>;
}

pub struct DbConn {
    client: postgres::Client,
}

impl DbConn {
    pub fn connect(connection_string: &str) -> anyhow::Result<DbConn> {
        let client = postgres::Client::connect(connection_string, NoTls)?;
        Ok(DbConn { client })
    }

    pub fn transaction(&mut self) -> anyhow::Result<Transaction> {
        let transaction = self.client.transaction()?;
        Ok(Transaction { transaction })
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
}

pub struct Transaction<'a> {
    transaction: postgres::Transaction<'a>,
}

impl Transaction<'_> {
    pub fn commit(self) -> anyhow::Result<()> {
        self.transaction.commit()?;
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
}
