#![allow(elided_lifetimes_in_paths)]
#![allow(clippy::wildcard_imports)]
pub use sea_orm_migration::prelude::*;
mod m20220101_000001_users;

mod m20261015_134144_repos;
mod m20261015_134149_user_keys;
pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20220101_000001_users::Migration),
            Box::new(m20261015_134144_repos::Migration),
            Box::new(m20261015_134149_user_keys::Migration),
            // inject-above (do not remove this comment)
        ]
    }
}
