use serenity::all::ResolvedValue;
use serenity::builder::{CreateCommand, CreateCommandOption, CreateInteractionResponseMessage, CreateInteractionResponse};
use serenity::model::application::{CommandOptionType, ResolvedOption};
use crate::mtg::models::CARD_NAME_MAX_LEN;
use crate::mtg::search::search_collections;
use crate::mtg::community_decks::list_community_decks;
use crate::models::config::BotConfig;

pub async fn run<'a>(_options: &[ResolvedOption<'a>],config: &BotConfig) -> CreateInteractionResponse {
    for option in _options {
        if option.name == "collections" {
            if let ResolvedValue::SubCommandGroup(sub_commands) = &option.value {
                for sub_command in sub_commands {
                    if sub_command.name == "search" {
                        if let ResolvedValue::SubCommand(inner_options) = &sub_command.value {
                            for inner_option in inner_options {
                                if inner_option.name == "name" {
                                    if let ResolvedValue::String(value) = &inner_option.value {
                                        return search_collections(value.to_string(),&config).await;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } else if option.name == "community_decks" {
            if let ResolvedValue::SubCommandGroup(sub_commands) = &option.value {
                for sub_command in sub_commands {
                    if sub_command.name == "list" {
                        return list_community_decks(&config).await;
                    }
                }
            }
        }
    }

    CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Command not implemented :("))
}

pub fn register() -> CreateCommand {
    CreateCommand::new("mtg")
        .description("Commands related to Magic: The Gathering")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommandGroup,
                "collections",
                "Commands related to FB3K card collections"
            ).add_sub_option(
                CreateCommandOption::new(
                    CommandOptionType::SubCommand,
                    "search",
                    "Search collections of all FB3K users for a given card"
                ).add_sub_option(
                    CreateCommandOption::new(
                        CommandOptionType::String,
                        "name",
                        "Name (or partial name) of card to search for"
                    )
                    .max_length(CARD_NAME_MAX_LEN)
                    .required(true)
                )
            )
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommandGroup,
                "community_decks",
                "Commands related to FB3K community decks"
            ).add_sub_option(
                CreateCommandOption::new(
                    CommandOptionType::SubCommand,
                    "list",
                    "List all known community decks"
                )
            )
        )
}