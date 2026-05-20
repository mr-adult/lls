use serde_json::{Map, Value};
use sqlx::PgPool;
use tracing::{Event, Level, field::Visit, span};
use tracing_subscriber::{Layer, layer::Context};

pub struct PostgresLayer(PgPool);

impl From<PgPool> for PostgresLayer {
    fn from(value: PgPool) -> Self {
        Self(value)
    }
}

struct PostgresFieldStorage(Value);

impl<S> Layer<S> for PostgresLayer
where
    S: tracing::Subscriber + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
        // Build our json object from the field values like we have been
        let mut fields = Map::new();
        let mut visitor = Visitor(&mut fields);
        attrs.record(&mut visitor);

        // And stuff it in our newtype.
        let storage = PostgresFieldStorage(Value::Object(fields));

        // Get a reference to the internal span data
        let span = ctx.span(id).unwrap();
        // Get the special place where tracing stores custom data
        let mut extensions = span.extensions_mut();
        // And store our data
        extensions.insert::<PostgresFieldStorage>(storage);
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        // The fields of the event
        let mut fields = Map::new();
        let mut visitor = Visitor(&mut fields);
        event.record(&mut visitor);

        let get_id_field_from_value = |value: &Value, field_id: &str| match value {
            Value::Null => None,
            Value::Bool(_) => panic!("{field_id} should never be a bool"),
            Value::Number(number) => number.as_i64(),
            Value::String(id) => id.parse::<i64>().ok(),
            Value::Array(_) => panic!("{field_id} should never be an object"),
            Value::Object(_) => panic!("{field_id} should never be an array"),
        };

        let get_id_field = |field_id: &str| {
            fields
                .get(field_id)
                .map(|value| get_id_field_from_value(value, field_id))
                .flatten()
        };

        let mut session_id = get_id_field("session_id");
        let mut request_id = get_id_field("request_id");
        let mut response_id = get_id_field("response_id");
        let mut notification_id = get_id_field("notification_id");

        // All of the span context
        let scope = ctx.event_scope(event);
        if let Some(scope) = scope {
            for (i, span) in scope.from_root().enumerate() {
                let extensions = span.extensions();
                let storage = extensions
                    .get::<PostgresFieldStorage>()
                    .expect("Did not get a PostgresFieldStorage");
                let mut field_data: Value = storage.0.clone();

                if session_id.is_none() {
                    if let Value::Object(map) = &mut field_data {
                        session_id = map
                            .get("session_id")
                            .map(|value| get_id_field_from_value(value, "session_id"))
                            .flatten()
                    }
                }

                if request_id.is_none() {
                    if let Value::Object(map) = &mut field_data {
                        request_id = map
                            .get("request_id")
                            .map(|value| get_id_field_from_value(value, "session_id"))
                            .flatten()
                    }
                }

                if response_id.is_none() {
                    if let Value::Object(map) = &mut field_data {
                        response_id = map
                            .get("response_id")
                            .map(|value| get_id_field_from_value(value, "session_id"))
                            .flatten()
                    }
                }

                if notification_id.is_none() {
                    if let Value::Object(map) = &mut field_data {
                        notification_id = map
                            .get("notification_id")
                            .map(|value| get_id_field_from_value(value, "session_id"))
                            .flatten()
                    }
                }

                let level = match span.metadata().level() {
                    &Level::TRACE => 0,
                    &Level::DEBUG => 1,
                    &Level::INFO => 2,
                    &Level::WARN => 3,
                    &Level::ERROR => 4,
                };

                let span_name = span.name().to_string();
                let pool = self.0.clone();
                tokio::spawn(async move {
                    sqlx::query_scalar::<_, ()>(
                "INSERT INTO log_spans (index, name, level, fields) VALUES ($1, $2, $3, $4)",
                    )
                    .bind(i as i32)
                    .bind(span_name)
                    .bind(level)
                    .bind(field_data.clone())
                    .fetch_optional(&pool)
                    .await
                    .expect("failed to log to postgres");

                    println!("logged to postgres");
                });
            }
        }

        let pool = self.0.clone();
        let message = if let Some(Value::String(message)) = fields.remove("message") {
            message
        } else {
            panic!("No message provided in an event.");
        };

        tokio::spawn(async move {
            sqlx::query_scalar::<_, ()>(
                "INSERT INTO logs (session_id, request_id, response_id, notification_id, time_stamp, message, fields) VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(session_id)
            .bind(request_id)
            .bind(response_id)
            .bind(notification_id)
            .bind(time::OffsetDateTime::now_utc())
            .bind(message)
            .bind(Value::Object(fields))
            .fetch_optional(&pool)
            .await
            .expect("failed to log to postgres");

            println!("logged to postgres");
        });
    }
}

struct Visitor<'a>(&'a mut Map<String, serde_json::Value>);

impl<'a> Visit for Visitor<'a> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn core::fmt::Debug) {
        self.0.insert(
            field.name().to_string(),
            serde_json::json!(format!("{value:?}")),
        );
    }

    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.0
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.0
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.0
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_i128(&mut self, field: &tracing::field::Field, value: i128) {
        self.0
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_u128(&mut self, field: &tracing::field::Field, value: u128) {
        self.0
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.0
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_bytes(&mut self, field: &tracing::field::Field, value: &[u8]) {
        self.0
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_error(
        &mut self,
        field: &tracing::field::Field,
        value: &(dyn std::error::Error + 'static),
    ) {
        self.0.insert(
            field.name().to_string(),
            serde_json::json!(format!("{}", value)),
        );
    }
}
