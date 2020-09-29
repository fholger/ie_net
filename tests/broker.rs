mod common;

use crate::common::TestBroker;
use ie_net::broker::user::Location;
use ie_net::messages::client_command::ClientCommand;

#[tokio::test]
async fn new_user_should_join_general_channel() {
    let mut broker = TestBroker::new();
    let mut client = broker.new_client("foo").await;
    broker.shutdown().await;
    client.process_messages().await;

    client.should_have_channel("General");
    client.should_be_in(&Location::Channel {
        name: "General".to_string(),
    });
}

#[tokio::test]
async fn join_channel() {
    let mut broker = TestBroker::new();
    let mut client = broker.new_client("foo").await;
    broker
        .send_command(
            &client,
            ClientCommand::Join {
                channel: "MyChannel".to_string(),
            },
        )
        .await;
    broker.shutdown().await;
    client.process_messages().await;

    client.should_have_channel("MyChannel");
    client.should_not_have_channel("General");
    client.should_be_in(&Location::Channel {
        name: "MyChannel".to_string(),
    });
}
