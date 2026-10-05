//! The small console command syntax, shared with client help and server validation.

pub const MAX_ADMIN_COMMAND_BYTES: usize = 256;
pub const ADMIN_COMMAND_HELP: &str = "help — show commands\n\
teleport X Y Z — teleport yourself to coordinates\n\
teleport PLAYER X Y Z — teleport a player to coordinates\n\
teleport DESTINATION — teleport yourself beside a player\n\
teleport PLAYER DESTINATION — teleport a player beside another\n\
tp is an alias for teleport. Coordinates are meters (X Y Z, Y is height).\n\
Player names match exactly, ignoring case. Quote names with spaces: teleport \"Ian Koski\" Violet.";

#[derive(Debug, Clone, PartialEq)]
pub enum AdminCommand {
    Help,
    Teleport {
        /// None means the player who entered the command.
        player: Option<String>,
        destination: TeleportDestination,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum TeleportDestination {
    Coordinates([f32; 3]),
    Player(String),
}

pub fn parse_admin_command(command: &str) -> Result<AdminCommand, String> {
    if command.len() > MAX_ADMIN_COMMAND_BYTES {
        return Err(format!(
            "Commands may contain at most {MAX_ADMIN_COMMAND_BYTES} bytes"
        ));
    }
    if command.chars().any(|ch| ch.is_control() && ch != '\t') {
        return Err("Enter one command on a single line".into());
    }
    let words = command_words(command)?;
    let Some(name) = words.first() else {
        return Err("Enter a command. Type help to see the available commands".into());
    };
    let name = name.strip_prefix('/').unwrap_or(name).to_ascii_lowercase();
    match (name.as_str(), &words[1..]) {
        ("help", []) => Ok(AdminCommand::Help),
        ("help", _) => Err("Usage: help".into()),
        ("teleport" | "tp", [destination]) => Ok(AdminCommand::Teleport {
            player: None,
            destination: TeleportDestination::Player(destination.clone()),
        }),
        ("teleport" | "tp", [player, destination]) => Ok(AdminCommand::Teleport {
            player: Some(player.clone()),
            destination: TeleportDestination::Player(destination.clone()),
        }),
        ("teleport" | "tp", [x, y, z]) => Ok(AdminCommand::Teleport {
            player: None,
            destination: TeleportDestination::Coordinates(coordinates(x, y, z)?),
        }),
        ("teleport" | "tp", [player, x, y, z]) => Ok(AdminCommand::Teleport {
            player: Some(player.clone()),
            destination: TeleportDestination::Coordinates(coordinates(x, y, z)?),
        }),
        ("teleport" | "tp", _) => Err(
            "Usage: teleport [PLAYER] X Y Z, or teleport [PLAYER] DESTINATION. Quote names with spaces".into(),
        ),
        _ => Err(format!("Unknown command: {name}. Type help to see the available commands")),
    }
}

fn coordinates(x: &str, y: &str, z: &str) -> Result<[f32; 3], String> {
    let mut position = [0.0; 3];
    for (index, coordinate) in [x, y, z].into_iter().enumerate() {
        position[index] = coordinate
            .parse::<f32>()
            .ok()
            .filter(|value| value.is_finite())
            .ok_or_else(|| "Coordinates must be three finite numbers: X Y Z".to_owned())?;
    }
    Ok(position)
}

fn command_words(command: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut started = false;
    let mut escaped = false;
    for ch in command.chars() {
        if escaped {
            word.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
            started = true;
        } else if let Some(delimiter) = quote {
            if ch == delimiter {
                quote = None;
            } else {
                word.push(ch);
            }
        } else if ch == '"' || ch == '\'' {
            quote = Some(ch);
            started = true;
        } else if ch.is_whitespace() {
            if started {
                if word.is_empty() {
                    return Err("Player names cannot be empty".into());
                }
                words.push(std::mem::take(&mut word));
                started = false;
            }
        } else {
            word.push(ch);
            started = true;
        }
    }
    if quote.is_some() || escaped {
        return Err("Finish the quoted name or escaped character".into());
    }
    if started {
        if word.is_empty() {
            return Err("Player names cannot be empty".into());
        }
        words.push(word);
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_help_aliases_coordinates_and_quoted_player_names() {
        assert_eq!(parse_admin_command(" /HELP ").unwrap(), AdminCommand::Help);
        assert_eq!(
            parse_admin_command("tp -10 20.5 3e1").unwrap(),
            AdminCommand::Teleport {
                player: None,
                destination: TeleportDestination::Coordinates([-10.0, 20.5, 30.0]),
            }
        );
        assert_eq!(
            parse_admin_command("teleport 'Ian Koski' \"Violet Koski\"").unwrap(),
            AdminCommand::Teleport {
                player: Some("Ian Koski".into()),
                destination: TeleportDestination::Player("Violet Koski".into()),
            }
        );
        assert_eq!(
            parse_admin_command("teleport Ian 1 2 3").unwrap(),
            AdminCommand::Teleport {
                player: Some("Ian".into()),
                destination: TeleportDestination::Coordinates([1.0, 2.0, 3.0]),
            }
        );
        assert_eq!(
            parse_admin_command("teleport Violet").unwrap(),
            AdminCommand::Teleport {
                player: None,
                destination: TeleportDestination::Player("Violet".into()),
            }
        );
    }

    #[test]
    fn malformed_or_unbounded_commands_and_nonfinite_coordinates_are_rejected() {
        for text in [
            "",
            "help Ian",
            "teleport",
            "tp 1 2 3 4 5",
            "tp x 2 3",
            "tp NaN 2 3",
            "tp inf 2 3",
            "tp 1e80 2 3",
            "tp \"Ian",
            "tp ''",
            "tp Ian\\",
            "help\nteleport Violet",
        ] {
            assert!(parse_admin_command(text).is_err(), "Accepted {text:?}");
        }
        assert!(parse_admin_command(&"a".repeat(MAX_ADMIN_COMMAND_BYTES + 1)).is_err());
    }
}
