use futures::future;
use lsp_server::{Message, Notification, Request, RequestId, Response};
use sqlx::PgPool;
use time::OffsetDateTime;

use crate::message::Conversation;

#[derive(Clone)]
pub(crate) struct MessageWithTimeStamp {
    pub(crate) time_stamp: OffsetDateTime,
    pub(crate) message: Message,
}

#[derive(Clone, Copy)]
pub(crate) enum MessageSource {
    Client,
    Server,
}

impl MessageSource {
    pub(crate) fn other(&self) -> Self {
        match self {
            MessageSource::Client => MessageSource::Server,
            MessageSource::Server => MessageSource::Client,
        }
    }
}

pub(crate) async fn get_all_messages_for_session_in_chronological_order(
    db: &PgPool,
    session_id: i64,
) -> Result<Conversation, sqlx::Error> {
    let requests = sqlx::query!(
        "SELECT * FROM requests WHERE session_id = $1 ORDER BY time_stamp ASC",
        session_id
    )
    .fetch_all(db);

    let responses = sqlx::query!(
        "SELECT * FROM responses WHERE session_id = $1 ORDER BY time_stamp ASC",
        session_id
    )
    .fetch_all(db);

    let notifications = sqlx::query!(
        "SELECT * FROM notifications WHERE session_id = $1 ORDER BY time_stamp ASC",
        session_id
    )
    .fetch_all(db);

    let (requests_result, responses_result, notifications_result) =
        future::join3(requests, responses, notifications).await;

    let requests = requests_result?;
    let responses = responses_result?;
    let notifications = notifications_result?;

    let requests_ref = &requests;
    let mut all_messages = responses
        .into_iter()
        .map(|response_record| {
            let id = RequestId::from(
                requests_ref
                    .iter()
                    .find(|request| request.id == response_record.id)
                    .expect("all responses to have a corresponding request")
                    .request_id
                    .clone(),
            );
            MessageWithTimeStamp {
                time_stamp: response_record.time_stamp,
                message: Message::Response(if response_record.is_error {
                    Response::new_err(
                        id,
                        response_record
                            .error_code
                            .expect("error_code to have a value when is_error is true"),
                        response_record.error_message.unwrap_or_default(),
                    )
                } else {
                    Response::new_ok(id, response_record.result)
                }),
            }
        })
        .collect::<Vec<_>>();

    all_messages.extend(
        requests
            .into_iter()
            .map(|request_record| MessageWithTimeStamp {
                time_stamp: request_record.time_stamp,
                message: Message::Request(Request::new(
                    RequestId::from(request_record.request_id),
                    request_record.method,
                    request_record.params,
                )),
            }),
    );

    all_messages.extend(
        notifications
            .into_iter()
            .map(|notification| MessageWithTimeStamp {
                time_stamp: notification.time_stamp,
                message: Message::Notification(Notification::new(
                    notification.method,
                    notification.params,
                )),
            }),
    );

    all_messages.sort_by(|message_with_time_stamp1, message_with_time_stamp2| {
        message_with_time_stamp1
            .time_stamp
            .cmp(&message_with_time_stamp2.time_stamp)
    });

    Ok(all_messages.into())
}
