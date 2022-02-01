use colored::Colorize;
use postgres::{Client, NoTls};
use reshape::{migrations::Migration, Reshape};

pub struct Test<'a> {
    name: &'a str,
    reshape: Reshape,
    old_db: Client,
    new_db: Client,

    first_migration: Option<Migration>,
    second_migration: Option<Migration>,
    expect_failure: bool,

    after_first_fn: Option<fn(&mut Client) -> ()>,
    intermediate_fn: Option<fn(&mut Client, &mut Client) -> ()>,
    after_completion_fn: Option<fn(&mut Client) -> ()>,
    after_abort_fn: Option<fn(&mut Client) -> ()>,
}

impl Test<'_> {
    pub fn new<'a>(name: &'a str) -> Test<'a> {
        let connection_string = std::env::var("POSTGRES_CONNECTION_STRING")
            .unwrap_or("postgres://postgres:postgres@localhost/reshape_test".to_string());

        let old_db = Client::connect(&connection_string, NoTls).unwrap();
        let new_db = Client::connect(&connection_string, NoTls).unwrap();

        let reshape = Reshape::new(&connection_string).unwrap();

        Test {
            name,
            reshape,
            old_db,
            new_db,
            first_migration: None,
            second_migration: None,
            expect_failure: false,
            after_first_fn: None,
            intermediate_fn: None,
            after_completion_fn: None,
            after_abort_fn: None,
        }
    }

    pub fn first_migration(&mut self, migration: &str) -> &mut Self {
        self.first_migration = Some(Self::parse_migration(migration));
        self
    }

    #[allow(dead_code)]
    pub fn second_migration(&mut self, migration: &str) -> &mut Self {
        self.second_migration = Some(Self::parse_migration(migration));
        self
    }

    #[allow(dead_code)]
    pub fn after_first(&mut self, f: fn(&mut Client) -> ()) -> &mut Self {
        self.after_first_fn = Some(f);
        self
    }

    #[allow(dead_code)]
    pub fn intermediate(&mut self, f: fn(&mut Client, &mut Client) -> ()) -> &mut Self {
        self.intermediate_fn = Some(f);
        self
    }

    #[allow(dead_code)]
    pub fn after_completion(&mut self, f: fn(&mut Client) -> ()) -> &mut Self {
        self.after_completion_fn = Some(f);
        self
    }

    #[allow(dead_code)]
    pub fn after_abort(&mut self, f: fn(&mut Client) -> ()) -> &mut Self {
        self.after_abort_fn = Some(f);
        self
    }

    #[allow(dead_code)]
    pub fn expect_failure(&mut self) {
        self.expect_failure = true;
    }

    fn parse_migration(encoded: &str) -> Migration {
        toml::from_str(encoded).unwrap()
    }
}

enum RunType {
    Simple,
    Completion,
    Abort,
}

impl Test<'_> {
    #[allow(dead_code)]
    pub fn run(&mut self) {
        if self.second_migration.is_some() {
            // Run to completion
            print_heading(&format!("Test completion: {}", self.name));
            self.run_internal(RunType::Completion);

            // Run and abort
            print_heading(&format!("Test abort: {}", self.name));
            self.run_internal(RunType::Abort);
        } else {
            print_heading(&format!("Test: {}", self.name));
            self.run_internal(RunType::Simple);
        }
    }

    fn run_internal(&mut self, run_type: RunType) {
        print_subheading("Clearing database");
        self.reshape.remove().unwrap();

        // Apply first migration, will automatically complete
        print_subheading("Applying first migration");
        let first_migration = self
            .first_migration
            .as_ref()
            .expect("no starting migration set");
        self.reshape.migrate(vec![first_migration.clone()]).unwrap();

        // Update search path
        self.old_db
            .simple_query(&reshape::schema_query_for_migration(&first_migration.name))
            .unwrap();

        // Automatically complete first migration
        self.reshape.complete().unwrap();

        // Run setup function
        if let Some(after_first_fn) = self.after_first_fn {
            print_subheading("Running setup and first checks");
            after_first_fn(&mut self.old_db);
            print_success();
        }

        // Apply second migration
        if let Some(second_migration) = &self.second_migration {
            if self.expect_failure {
                print_subheading("Applying second migration (expecting failure)");
                let result = self
                    .reshape
                    .migrate(vec![first_migration.clone(), second_migration.clone()]);

                if result.is_ok() {
                    panic!("expected second migration to fail");
                }
            } else {
                print_subheading("Applying second migration");
                self.reshape
                    .migrate(vec![first_migration.clone(), second_migration.clone()])
                    .unwrap();
            }

            // Update search path
            self.new_db
                .simple_query(&reshape::schema_query_for_migration(&second_migration.name))
                .unwrap();

            if let Some(intermediate_fn) = self.intermediate_fn {
                print_subheading("Running intermediate checks");
                intermediate_fn(&mut self.old_db, &mut self.new_db);
                print_success();
            }

            match run_type {
                RunType::Completion => {
                    print_subheading("Completing");
                    self.reshape.complete().unwrap();

                    if let Some(after_completion_fn) = self.after_completion_fn {
                        print_subheading("Running post-completion checks");
                        after_completion_fn(&mut self.new_db);
                        print_success();
                    }
                }
                RunType::Abort => {
                    print_subheading("Aborting");
                    self.reshape.abort().unwrap();

                    if let Some(after_abort_fn) = self.after_abort_fn {
                        print_subheading("Running post-abort checks");
                        after_abort_fn(&mut self.old_db);
                        print_success();
                    }
                }
                _ => {}
            }
        }

        print_subheading("Checking cleanup");
        assert_cleaned_up(&mut self.new_db);
        print_success();
    }
}

fn print_heading(text: &str) {
    let delimiter = std::iter::repeat("=").take(80).collect::<String>();

    println!();
    println!();
    println!("{}", delimiter.blue().bold());
    println!("{}", add_spacer(text, "=").blue().bold());
    println!("{}", delimiter.blue().bold());
}

fn print_subheading(text: &str) {
    println!();
    println!("{}", add_spacer(text, "=").blue());
}

fn print_success() {
    println!("{}", add_spacer("Success", "=").green());
}

fn add_spacer(text: &str, char: &str) -> String {
    const TARGET_WIDTH: usize = 80;
    let num_of_chars = (TARGET_WIDTH - text.len() - 2) / 2;
    let spacer = std::iter::repeat(char)
        .take(num_of_chars)
        .collect::<String>();

    let extra = if text.len() % 2 == 0 { "" } else { char };

    format!("{spacer} {text} {spacer}{extra}", spacer = spacer)
}

pub fn assert_cleaned_up(db: &mut Client) {
    // Make sure no temporary columns remain
    let temp_columns: Vec<String> = db
        .query(
            "
            SELECT column_name
            FROM information_schema.columns
            WHERE table_schema = 'public'
            AND column_name LIKE '__reshape%'
            ",
            &[],
        )
        .unwrap()
        .iter()
        .map(|row| row.get(0))
        .collect();

    assert!(
        temp_columns.is_empty(),
        "expected no temporary columns to exist, found: {}",
        temp_columns.join(", ")
    );

    // Make sure no triggers remain
    let triggers: Vec<String> = db
        .query(
            "
            SELECT trigger_name
            FROM information_schema.triggers
            WHERE trigger_schema = 'public'
            AND trigger_name LIKE '__reshape%'
            ",
            &[],
        )
        .unwrap()
        .iter()
        .map(|row| row.get(0))
        .collect();

    assert!(
        triggers.is_empty(),
        "expected no triggers to exist, found: {}",
        triggers.join(", ")
    );

    // Make sure no functions remain
    let functions: Vec<String> = db
        .query(
            "
            SELECT routine_name
            FROM information_schema.routines
            WHERE routine_schema = 'public'
            AND routine_name LIKE '__reshape%'
            ",
            &[],
        )
        .unwrap()
        .iter()
        .map(|row| row.get(0))
        .collect();

    assert!(
        functions.is_empty(),
        "expected no functions to exist, found: {}",
        functions.join(", ")
    );
}
