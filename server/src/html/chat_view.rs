use std::collections::HashSet;

use lsp_server::Message;
use serde_json::Value;

use crate::{
    message::{Conversation, MessageKind, classify},
    session::MessageSource,
    utils::get_iso_string,
};

pub(crate) fn append_chat_html_to(
    html: &mut String,
    conversation: &Conversation,
    allow_list: &HashSet<Option<MessageKind>>,
) {
    html.push_str("<div id=\"chat\">");
    {
        for message_with_time_stamp in conversation {
            if !allow_list.contains(&classify(&message_with_time_stamp.message, conversation)) {
                continue;
            }

            let source = crate::message::get_source(&message_with_time_stamp.message, conversation);
            let message = &message_with_time_stamp.message;

            let class_name;
            let message_wrapper_class;
            match source {
                Some(MessageSource::Client) => {
                    class_name = "client_message";
                    message_wrapper_class = "client_message_wrapper";
                }
                Some(MessageSource::Server) => {
                    class_name = "server_message";
                    message_wrapper_class = "server_message_wrapper";
                }
                None => {
                    class_name = "message";
                    message_wrapper_class = "message_wrapper";
                }
            };

            html.push_str("<div class=\"");
            html.push_str(message_wrapper_class);
            html.push_str("\">");
            {
                html.push_str("<div class=\"");
                {
                    html.push_str(class_name);
                    html.push_str("\">");
                    html.push_str("<details class=\"message_summary");
                    match source {
                        Some(MessageSource::Client) => html.push_str(" client"),
                        Some(MessageSource::Server) => html.push_str(" server"),
                        None => {}
                    }
                    html.push_str("\">");
                    {
                        html.push_str("<summary>");
                        match &message {
                            Message::Request(req) => {
                                html.push_str("Request: ");
                                html.push_str(&req.method);
                            }
                            Message::Response(resp) => {
                                html.push_str("Response: ");
                                let method = conversation
                                    .requests()
                                    .get(&resp.id)
                                    .map(|request| &request.method);
                                if let Some(method) = method {
                                    html.push_str(method);
                                } else {
                                    html.push_str("Unknown Response");
                                }
                            }
                            Message::Notification(not) => {
                                html.push_str("Notification: ");
                                html.push_str(&not.method);
                            }
                        }

                        html.push_str("</summary>");
                        {
                            append_json_html_to(
                                html,
                                serde_json::to_value(message.clone()).unwrap(),
                            );
                        }
                    }
                    html.push_str("</details>");
                }
                html.push_str("</div>");

                html.push_str("<span class=\"timestamp\">");
                html.push_str(&get_iso_string(&message_with_time_stamp.time_stamp));
                html.push_str("</span>");
            }
            html.push_str("</div>");
        }
    }
    html.push_str("</div>");
}

fn append_json_html_to(html: &mut String, value: Value) {
    match value {
        Value::Null => {
            html.push_str("<span style=\"color: lightblue\">null</span>");
            html.push_str("<br/>");
        }
        Value::Bool(value) => {
            if value {
                html.push_str("<span style=\"color: lightblue\">true</span>");
            } else {
                html.push_str("<span style=\"color: lightblue\">false</span>");
            }
            html.push_str("<br/>");
        }
        Value::Number(number) => {
            html.push_str(&number.to_string());
            html.push_str("<br/>");
        }
        Value::String(str) => {
            html.push('"');
            html.push_str(&html_escape::encode_text(&str));
            html.push('"');
            html.push_str("<br/>");
        }
        Value::Array(values) => {
            if values.is_empty() {
                html.push_str("[]");
            } else {
                html.push_str("<details open class=\"array_container\">");
                html.push_str("<summary>[]</summary>");
                html.push_str("<div class=\"array_content\">");
                for value in values {
                    append_json_html_to(html, value);
                }
                html.push_str("</div>");
                html.push_str("</details>");
            }
        }
        Value::Object(map) => {
            if map.is_empty() {
                html.push_str("{}");
            } else {
                html.push_str("<details open class=\"object_container\">");
                html.push_str("<summary>{}</summary>");
                html.push_str("<div class=\"object_content\">");
                for kvp in map {
                    append_json_kvp_to(html, kvp);
                }
                html.push_str("</div>");
                html.push_str("</details>");
            }
        }
    }
}

fn append_json_kvp_to(html: &mut String, kvp: (String, Value)) {
    match kvp.1 {
        Value::Null => {
            html.push('"');
            html.push_str(&html_escape::encode_text(&kvp.0));
            html.push('"');
            html.push_str(": ");
            html.push_str("<span style=\"color: lightblue\">null</span>");
            html.push_str("<br/>");
        }
        Value::Bool(value) => {
            html.push('"');
            html.push_str(&html_escape::encode_text(&kvp.0));
            html.push('"');
            html.push_str(": ");
            if value {
                html.push_str("<span style=\"color: lightblue\">true</span>");
            } else {
                html.push_str("<span style=\"color: lightblue\">false</span>");
            }
            html.push_str("<br/>");
        }
        Value::Number(num) => {
            html.push('"');
            html.push_str(&html_escape::encode_text(&kvp.0));
            html.push('"');
            html.push_str(": ");
            html.push_str(&num.to_string());
            html.push_str("<br/>");
        }
        Value::String(str) => {
            html.push('"');
            html.push_str(&html_escape::encode_text(&kvp.0));
            html.push('"');
            html.push_str(": ");
            html.push('"');
            html.push_str(&html_escape::encode_text(&str));
            html.push('"');
            html.push_str("<br/>");
        }
        Value::Array(values) => {
            if !values.is_empty() {
                html.push_str("<details class=\"array_container\">");
                html.push_str("<summary>");
            }
            html.push('"');
            html.push_str(&html_escape::encode_text(&kvp.0));
            html.push('"');
            html.push_str(": []");
            if !values.is_empty() {
                html.push_str("</summary>");
                html.push_str("<div class=\"array_content\">");
                for value in values {
                    append_json_html_to(html, value);
                }
                html.push_str("</div>");
                html.push_str("</details>");
            }
        }
        Value::Object(object) => {
            if !object.is_empty() {
                html.push_str("<details class=\"object_container\">");
                html.push_str("<summary>");
            }
            html.push('"');
            html.push_str(&html_escape::encode_text(&kvp.0));
            html.push('"');
            html.push_str(": {}");
            if !object.is_empty() {
                html.push_str("</summary>");
                html.push_str("<div class=\"object_content\">");
                for kvp in object {
                    append_json_kvp_to(html, kvp);
                }
                html.push_str("</div>");
                html.push_str("</details>");
            }
        }
    }
}
