use clap::{Parser, Subcommand};
use vulture::manual::builders;

#[derive(Parser)]
#[command(
    name = "vulture-dotgraph",
    about = "Render the bench-harness synthetic transit networks as Graphviz DOT"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Single route with N stops and M trips
    Linear {
        #[arg(long, short)]
        stops: usize,
        #[arg(long, short)]
        trips: usize,
    },
    /// Grid of horizontal routes with vertical connectors
    Grid {
        #[arg(long, short)]
        routes: usize,
        #[arg(long, short = 'c')]
        stops_per_route: usize,
        #[arg(long)]
        connectors: usize,
    },
    /// Hub-and-spoke network with footpath-connected hubs
    HubSpoke {
        #[arg(long, short)]
        hubs: usize,
        #[arg(long, short = 'r')]
        routes_per_hub: usize,
        #[arg(long, short = 's')]
        stops_per_spoke: usize,
    },
    /// Chain of single-leg routes forcing transfers
    Chain {
        #[arg(long, short)]
        segments: usize,
    },
    /// Parallel paths from source to target with varying legs
    ParallelPaths {
        #[arg(long, short = 'p')]
        path_count: usize,
        #[arg(long, short = 'm')]
        max_legs: usize,
    },
}

fn main() {
    let cli = Cli::parse();

    let (tt, name) = match cli.command {
        Command::Linear { stops, trips } => {
            (builders::build_linear(stops, trips), "linear".to_string())
        }
        Command::Grid {
            routes,
            stops_per_route,
            connectors,
        } => (
            builders::build_grid(routes, stops_per_route, connectors),
            "grid".to_string(),
        ),
        Command::HubSpoke {
            hubs,
            routes_per_hub,
            stops_per_spoke: stop_per_spoke,
        } => (
            builders::build_hub_spoke(hubs, routes_per_hub, stop_per_spoke),
            "hub_spoke".to_string(),
        ),
        Command::Chain { segments } => (builders::build_chain(segments), "chain".to_string()),
        Command::ParallelPaths {
            path_count,
            max_legs,
        } => (
            builders::build_parallel_paths(path_count, max_legs),
            "parallel_paths".to_string(),
        ),
    };

    let dot = tt.to_dot(&name).expect("failed to generate DOT graph");
    print!("{dot}");
}
