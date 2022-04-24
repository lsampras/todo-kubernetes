psql -h $SQL_SERVER -p $SQL_PORT -U $PGUSER -f migration.sql
export DATABASE_URL=postgres://$PGUSER:$PGPASSWORD@$SQL_SERVER:$SQL_PORT/postgres
cargo run