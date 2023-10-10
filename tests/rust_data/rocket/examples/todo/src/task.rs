use rocket::serde::Serialize;
use diesel::{self, result::QueryResult, prelude::*};

mod schema {
    table! {
        tasks {
            id -> Nullable<Integer>,
            description -> Text,
            completed -> Bool,
        }
    }
}

use self::schema::tasks;

use crate::DbConn;

#[derive(Serialize, Queryable, Insertable, Debug, Clone)]
#[serde(crate = "rocket::serde")]
#[diesel(table_name = tasks)]
pub struct Task {
    #[serde(skip_deserializing)]
    pub id: Option<i32>,
    pub description: String,
    pub completed: bool
}

#[derive(Debug, FromForm)]
pub struct Todo {
    pub description: String,
}

impl Task {
    pub async fn all(conn: &DbConn) -> QueryResult<Vec<Task>> {
        conn.run(|c| {
            tasks::table.order(tasks::id.desc()).load::<Task>(c)
        }).await
    }

    /// Returns the number of affected rows: 1.
    pub async fn insert(todo: Todo, conn: &DbConn) -> QueryResult<usize> {
        conn.run(|c| {
            let t = Task { id: None, description: todo.description, completed: false };
            diesel::insert_into(tasks::table).values(&t).execute(c)
        }).await
    }

    /// Returns the number of affected rows: 1.
    pub async fn toggle_with_id(id: i32, conn: &DbConn) -> QueryResult<usize> {
        conn.run(move |c| {
            let task = tasks::table.filter(tasks::id.eq(id)).get_result::<Task>(c)?;
            let new_status = !task.completed;
            let updated_task = diesel::update(tasks::table.filter(tasks::id.eq(id)));
            updated_task.set(tasks::completed.eq(new_status)).execute(c)
        }).await
    }

    /// Returns the number of affected rows: 1.
    pub async fn delete_with_id(id: i32, conn: &DbConn) -> QueryResult<usize> {
        conn.run(move |c| diesel::delete(tasks::table)
            .filter(tasks::id.eq(id))
            .execute(c))
            .await
    }

    /// Returns the number of affected rows.
    #[cfg(test)]
    pub async fn delete_all(conn: &DbConn) -> QueryResult<usize> {
        conn.run(|c| diesel::delete(tasks::table).execute(c)).await
    }
}
