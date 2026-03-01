use clap::{Parser, Subcommand};
use raptor::simple::builders;

#[derive(Parser)]
#[command(name = "raptor-dotgraph", about = "Generate DOT graphs of synthetic transit networks")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Single route with N stops and M trips
    Linear {
        #[arg(long)]
        stops: usize,
        #[arg(long)]
        trips: usize,
    },
    /// Grid of horizontal routes with vertical connectors
    Grid {
        #[arg(long)]
        routes: usize,
        #[arg(long)]
        stops_per_route: usize,
        #[arg(long)]
        connectors: usize,
    },
    /// Hub-and-spoke network with footpath-connected hubs
    HubSpoke {
        #[arg(long)]
        hubs: usize,
        #[arg(long)]
        routes_per_hub: usize,
        #[arg(long)]
        spokes: usize,
    },
    /// Chain of single-leg routes forcing transfers
    Chain {
        #[arg(long)]
        segments: usize,
    },
    /// Parallel paths from source to target with varying legs
    ParallelPaths {
        #[arg(long)]
        path_count: usize,
        #[arg(long)]
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
            spokes,
        } => (
            builders::build_hub_spoke(hubs, routes_per_hub, spokes),
            "hub_spoke".to_string(),
        ),
        Command::Chain { segments } => {
            (builders::build_chain(segments), "chain".to_string())
        }
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
