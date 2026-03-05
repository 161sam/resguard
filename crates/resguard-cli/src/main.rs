use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name="resguard")]
#[command(about="Linux resource guard using systemd slices")]
struct Cli {

    #[command(subcommand)]
    command: Commands,

}

#[derive(Subcommand)]
enum Commands {

    Status,

    Apply {
        profile: String,
        #[arg(long)]
        dry_run: bool
    },

    Run {
        #[arg(long)]
        class: String,
        command: Vec<String>
    }

}

fn main() {

    let cli = Cli::parse();

    match cli.command {

        Commands::Status => {
            println!("Resguard status (not implemented)");
        }

        Commands::Apply { profile, dry_run } => {
            println!("Apply profile {} dry_run={}", profile, dry_run);
        }

        Commands::Run { class, command } => {

            println!("Run in class {}", class);

            if command.is_empty() {
                println!("No command provided");
                return;
            }

            println!("Command: {:?}", command);

        }

    }

}