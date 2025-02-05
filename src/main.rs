use std::{env, error::Error, sync::Arc};
use dotenv::dotenv;
use tokio::sync::RwLock;

use rand::Rng;
use regex::Regex;

use twilight_gateway::{Event, EventTypeFlags, Intents, Shard, ShardId, StreamExt as _};
use twilight_http::Client;
use twilight_model::{
    gateway::payload::incoming::MessageCreate
};

//use tracing_subscriber::fmt::format::FmtSpan;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    //Set enviorment
    dotenv().ok();
    //tracing_subscriber::fmt::init();

    //Bot access token
    let token = env::var("DISCORD_TOKEN")?;
    //Read twilight docs, guild messages is the channel type, content needs to be applied in oath AND bot settings
    let intents = Intents::GUILD_MESSAGES | Intents::MESSAGE_CONTENT;
    //Shard for umm ???
    let mut shard = Shard::new(ShardId::ONE, token.clone(), intents);
    //Old setup without multithraded RWlock
    //let client = Client::new(token);
    // Creates a rwlock copy of client
    let client = Arc::new(RwLock::new(Client::new(token)));

    //Await shard
    while let Some(item) = shard.next_event(EventTypeFlags::all()).await {
        let Ok(event) = item else {
            continue;
        };
        // If shard event is MessageCreate (New Message) start check
        if let Event::MessageCreate(message_content) = event {
            // Old setup that just used this method
            //processMessage(&client, messageContent.0).await?;
            //Create arc rwlock clone
            let client_copy = Arc::clone(&client);
            //Remove MessageCreate from its box
            // This also seems to fix crashing when rate limited, I think it's because the box content isnt messing w/ it anymore
            let message_content_unboxed = *message_content;
            //Start async handling tokio
            tokio::spawn(async move {
                if let Err(_) = process_message(client_copy, message_content_unboxed).await {
                }
            });
        }
    }
    Ok(())
}
//Take the message event and check if it fits command syntax, if it does strip the command then send the numbers to the roller
async fn process_message(client: Arc<RwLock<Client>>, message_content: MessageCreate) -> Result<(), Box<dyn Error + Send + Sync>> {
// Old Client = client: &Client Old msg = msg: MessageCreate
    //Check for "/r " if so remove 3 characters
    if message_content.content.starts_with("/r ") || message_content.content.starts_with(".r ") || message_content.content.starts_with("!r ") {
        // New format for tokio spawn blocking
        let input = message_content.content[3..].to_string();
        let roll_task = tokio::task::spawn_blocking(move || roll_dice(&input));
        let roll = roll_task.await?;
        // Old Format, comment above, uncomment under
        //let roll = roll_dice(&msg.content[3..]);
        //Get the user to ping
        let user = message_content.author.id;
        //Format the response, @Author: roll
        let message_response = format!("<@{}>: {}", user, roll);
        //rwlock copy for client token
        let client = client.read().await;
        //Twilight create message
        client.create_message(message_content.channel_id)
            .content(&message_response)
            .await?;
    }
    else if message_content.content.starts_with("/roll ") || message_content.content.starts_with(".roll ") || message_content.content.starts_with("!roll "){
        let input = message_content.content[6..].to_string();
        let roll_task = tokio::task::spawn_blocking(move || roll_dice(&input));
        let roll = roll_task.await?;
        let user = message_content.author.id;
        let message_response = format!("<@{}>: {}", user, roll);
        let client = client.read().await;
        client.create_message(message_content.channel_id)
            .content(&message_response)
            .await?;
    }
    Ok(())
}
fn roll_dice(input: &str) -> String {
    //Start RNG
    let mut rng = rand::thread_rng();

    // search for #, then trim everything after the # for reuse later
    let (input_without_comment, comment) =
        if let Some(comment_position) = input.find('#') {
            let (input_before_comment, comment_part) = input.split_at(comment_position);
            (input_before_comment, comment_part[1..].trim().to_string())  // not sure if needed the string part
    }
        else {
            (input, "".to_string()) // not sure if needed
    };
    //Remove all spaces
    let input_clean = input_without_comment.replace(|c: char| c.is_whitespace(), "");
    //Fucks empty roll
    if input_clean.is_empty() {
        return "Roll can not be empty!".to_string();
    }
    //Basic help command
    if input_clean.starts_with("help"){
        return "/r [numOfDice]d[numSidesOfDice]".to_string();
    }


    // Detect and split dice expression from comparison part
    let comparison_operators = ["<=", ">=", "<", ">", "="];
    // Gets the pure dice combo
    let mut dice_expression = input_clean.as_str();
    // Gets what to compare to
    let mut comparison_section = "";
    for operator in &comparison_operators {
        if let Some(pos) = input_clean.find(operator) {
            dice_expression = &input_clean[..pos]; // command is before comparison
            comparison_section = &input_clean[pos..]; // operator and value to compare it to
            break;
        }
    }

    // Check if the input matches valid terms (dice or constants with optional signs)
    let regex_check = Regex::new(r"^([+-]?(\d*d\d+(!\d*)?|\d+)([+-](\d*d\d+(!\d*)?|\d+))*)$").unwrap();
    // If doesn't match regex syntax
    if !regex_check.is_match(&dice_expression) {
        return "Invalid character in command, accepted characters: [0-9],[+-!^],[d]".to_string();
    }

    // regex filter for split
    // i.e. Regex("([+-]?[^+-]+)")
    let regex_terms = Regex::new(r"([+-]?[^+-]+)").unwrap();
    // Split into individual terms with signs
    // i.e. ["1d20", "+3d30"]
    let split_dice: Vec<&str> = regex_terms.find_iter(dice_expression).map(|m| m.as_str()).collect();

    // Verify the split dice and reconstruct the cleaned input, just error checking
    // Also catches me if i fuck up the regex one day by adding a shit ton of shitty features
    let repaired_dice: String = split_dice.iter().copied().collect();
    if repaired_dice != dice_expression {
        return "Error in formatting".to_string();
    }

    // Prepare for output, formatted_parts is for the equation visual output string, and total_sum is the final output
    let mut formatted_parts = String::new();
    let mut total_sum = 0;
    let mut all_rolls = Vec::new();

    // Parse and enumerate over each indidivudal part of the split dice
    for (i, dice_segment) in split_dice.iter().enumerate() {
        // Parse the sign and value part from an individual segment
        // 1 = + -1 = -, nothing = 1 because its positive
        // dice_roll is the pure input without signs, i.e. -59d1 -> 59d1
        let (sign_character, dice_roll) =
            if dice_segment.starts_with('+') {
            (1, &dice_segment[1..])}
            else if dice_segment.starts_with('-') {
            (-1, &dice_segment[1..])}
            else {
            (1, *dice_segment)};



        // Check if num of dice is really a dice, because sidekick allows "math"
        // At the end of this formatted parts and total sum are combined, with the else of this statement combining the normal nums
        if dice_roll.contains('d') {
            // Split the dice roll into the total dice and sides of the dice being rolled
            let (num_of_dice, rest) = match dice_roll.split_once('d') {
                Some(parts) => parts,
                None => return "Error in roll formatting".to_string(),
            };
            let (num_of_sides, _explosion_condition) = if let Some(explosion_pos) = rest.find('!') {
                (&rest[..explosion_pos], Some(&rest[explosion_pos + 1..]))
            } else {
                (rest, None)
            };

            // Make sure total dice fits into u128 and sets empty to 1
            let dice = if num_of_dice.is_empty() {1}
                else {
                    match num_of_dice.parse::<u128>() {
                        Ok(n) => n,
                        Err(_) => return "Too many dice to count...".to_string()
                    }};
            // Make sure total sides fits into u128
            let sides = match num_of_sides.parse::<u128>() {
                Ok(s) => s,
                Err(_) => return "Too many sides to count...".to_string(),
            };

            let explosion_limit = 512;
            let mut explosion_count = 0;
            // Check if the dice roll has an explosion condition
            // doesnt work rn
            let explode_condition = if let Some(pos) = dice_roll.find('!') {
                dice_roll[pos+1..].to_string()
            }
            else {
                String::new()
            };
            let can_explode = dice_roll.chars().any(|c| c == '!');
            // rolling of dice
            let mut rolls: Vec<u128> = vec![];
            for _ in 0..dice {
                let mut roll = rng.gen_range(1..=sides);
                rolls.push(roll);
                while explosion_count < explosion_limit {
                    // Check explosion condition
                    let explode = if explode_condition.is_empty() {
                        roll == sides // Default: explode on max roll
                    } else if let Some(c) = explode_condition.chars().next() {
                        match c {
                            '<' => roll < explode_condition[1..].parse::<u128>().unwrap_or(1), // d20!<5 = explode below 5
                            '=' => roll == explode_condition[1..].parse::<u128>().unwrap_or(sides), // d20!=1 = explode on 1
                            _ => false
                        }
                    } else {
                        false
                    };
                    if can_explode {
                    // If explosion condition is met, roll again
                    if explode {
                        let new_roll = rng.gen_range(1..=sides);
                        rolls.push(new_roll);
                        roll = new_roll;
                        explosion_count += 1;
                    } else {
                        break; // Stop exploding
                    }
                    } else { break;}
                }
            }
            // Add  up the rolls
            let sum_of_rolls: u128 = rolls.iter().sum();
            // Add back the negative/positive
            let sign_applied_sum = sum_of_rolls as i128  * sign_character;

            // The text now, copying sidekick and making each roll visible
            let rolls_text = if rolls.is_empty() {
                String::from("")
            }
            else {
                rolls.iter().map(|r| r.to_string()).collect::<Vec<_>>().join("+")
            };
            // put em in parenthesis
            let parenthesis_rolls = if rolls.is_empty() {
                String::from("0")
            }
            else {
                format!("({})", rolls_text)
            };

            // add back the -/+ and apply negative if first
            let signed_parenthesis_rolls = if i == 0 {
                if sign_character == -1 {
                    format!("-{}", parenthesis_rolls)
                }
                else {
                    parenthesis_rolls
                }
            }
            else {
                if sign_character == 1 {
                    format!("+{}", parenthesis_rolls)
                }
                else {
                    format!("-{}", parenthesis_rolls)
                }
            };
            // Append roll text and sum
            formatted_parts.push_str(&signed_parenthesis_rolls);
            total_sum += sign_applied_sum;
            all_rolls.extend(rolls);
        }
        // If it isn't a dice, then it's a normal constant number
        else {
            // Make sure the constant can fit in a i128
            let constant = match dice_roll.parse::<i128>() {
                Ok(c) => c,
                Err(_) => return "Error number too big...".to_string(),
            };
            let signed_constant = constant * sign_character;
            // string display for signs copeid from above
            let formatted_part = if i == 0 {
                if sign_character == -1 {
                    format!("-{}", constant)
                }
                else {
                    dice_roll.to_string()
                }
            }
            else {
                if sign_character == 1 {
                    format!("+{}", constant)
                }
                else {
                    format!("-{}", constant)
                }
            };
            formatted_parts.push_str(&formatted_part);
            total_sum += signed_constant;
        }

    }
    // Final output ` ` around block like sidekick did
    format!("`{}` {} = {} = {}", input_clean, comment, formatted_parts, total_sum)
}
