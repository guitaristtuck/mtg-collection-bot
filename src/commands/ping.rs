use serenity::model::application::ResolvedOption;
use serenity::builder::{CreateCommand, CreateInteractionResponseMessage, CreateInteractionResponse};

pub fn run(_options: &[ResolvedOption]) -> CreateInteractionResponse {
    CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("F3BK bot reporting in, ready for fuckery"))
}

pub fn register() -> CreateCommand {
    CreateCommand::new("ping").description("Check to see if the server is alive")
}