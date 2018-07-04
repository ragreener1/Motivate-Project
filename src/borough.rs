extern crate rand;

use std;
use std::collections::HashMap;
use std::io::Write;
use itertools::Itertools;
use std::time::SystemTime;
use std::fs;
use std::io;
use rand::distributions;
use rand::distributions::Distribution;
use rand::{thread_rng};
use std::rc::Rc;
use std::sync::Arc;
use std::cell::RefCell;
use weather::Weather;
use transport_mode::TransportMode;
use journey_type::JourneyType;
use neighbourhood::Neighbourhood;
use subculture::Subculture;
use scenario::Scenario;
use agent::Agent;
use statistics;
use union_with::union_of;

/// A single borough that will run the simulation
pub struct Borough {
    /// The borough's id, used in the output file
    pub id: String,
    /// The scenario for the simulation
    pub scenario: Scenario,
    /// The number of years to run the simulation for
    pub total_years: u32,
    /// The total number of people in the simulation
    pub number_of_people: u32,
    /// How connected an agent should be to its social network
    pub social_connectivity: f32,
    /// How connected an agent should be to its subculture
    pub subculture_connectivity: f32,
    /// How connected an agent should be to its neighbourhood
    pub neighbourhood_connectivity: f32,
    /// The minimum number of links in the social network
    pub number_of_social_network_links: u32,
    /// The minimum number of links in the neighbourhood
    pub number_of_neighbour_links: u32,
    /// The number of days that account for approximately 86% of the habit average
    pub days_in_habit_average: u32,
    /// The weather pattern
    pub weather_pattern: HashMap<u32, Weather>
}

impl Borough {
    /// Run the simulation
    pub fn run(&mut self, network: HashMap<u32, Vec<u32>>) -> Result<(), io::Error>{
        // Used for monitoring running time
        let t0 = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        // Create the agents
        let mut residents: Vec<Rc<RefCell<Agent>>> = self.set_up(network);

        // Report the setup running time
        let t1 = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();
        println!("[{}] Agents created in {}s", self.id, t1-t0);

        // Create the output directory if it does not already exist
        fs::create_dir_all("output")?;

        // Create the output file, and write the header to it
        let mut file = fs::File::create(format!("output/output_{}.csv", self.id))?;
        file.write_all(generate_csv_header(&self.scenario).as_bytes())?;

        // Get the weather at day 0
        let mut weather = self.weather_pattern.get(&0).unwrap();

        // Write the first set of statistics to the file
        file.write_all(generate_csv_output(0, &weather, &self.scenario, &residents).as_bytes())?;

        // For each day in the simulation
        for day in 1..self.total_years * 365 {
            // Only consider weekdays
            if weekday(day) {
                // Log the day to the terminal
                println!("[{}] Day: {}", self.id, day);

                // Get the new weather
                let new_weather = self.weather_pattern.get(&day).unwrap();

                // For each resident update the habit
                for resident in residents.iter_mut() {
                    resident.borrow_mut().update_habit();
                }

                // For each resident, choose a travel mode
                for resident in residents.iter_mut() {
                    resident.borrow_mut().choose(new_weather, weather != new_weather);
                }

                // Update the weather
                weather = new_weather;

                // Log the stats to the file
                file.write_all(generate_csv_output(day, &weather, &self.scenario, &residents).as_bytes())?;
            }
        }

        // Output the running time to the terminal
        let t2 = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        println!("[{}] Total elapsed time {}s", self.id, t2 - t0);
        println!("[{}] Total elapsed time excl. set up {}s", self.id, t2 - t1);

        Ok(())

    }

    /// Create the agents
    /// Returns a set of Rc pointers, storing RefCells of Agents
    /// Rc are reference counter pointers, that are store immutable data,
    /// RefCells are immutable but have mutable contents
    /// Meaning that Agents are mutable
    fn set_up(&mut self, network: HashMap<u32, Vec<u32>>) -> Vec<Rc<RefCell<Agent>>> {
        // Create an empty vec to store agents
        let mut residents: Vec<Rc<RefCell<Agent>>> = Vec::new();
        // Create self.number_of_people unlinked agents
        for _ in 0..self.number_of_people {
            let agent: Agent = self.create_unlinked_agent();
            residents.push(Rc::new(RefCell::new(agent)));
        }

        // Generate social network
        // self.link_agents_to_social_network(&residents, self.number_of_social_network_links);
        link_agents_from_predefined_network(&mut residents, network);

        // Group agents by neighbourhood
        let neighbourhood_residents: HashMap<String, Vec<Rc<RefCell<Agent>>>> = residents
            .iter()
            .map(|x| (x.borrow().neighbourhood.id.clone(), x.clone()))
            .into_group_map();

        // For each neighbourhood create a social network
        for (_, v) in neighbourhood_residents {
            self.link_agents_to_neighbours(&v, self.number_of_neighbour_links);
        }

        // Return the created agents
        residents
    }

    /// Create an unlinked agent
    fn create_unlinked_agent(&self) -> Agent {
        // Choose a subculture, neighbourhood, and commute length
        let subculture = self.choose_subculture();
        let neighbourhood = self.choose_neighbourhood();
        let commute_length = self.choose_journey_type();

        // Weather sensitivity is currently fixed
        let weather_sensitivity = 0.9f32;
        // TODO: Should consistency be used
        let consistency = rand::random::<f32>();

        // TODO: Should suggestibility be used
        let suggestibility = random_normal(1.0, 0.25);

        // Choose the current_mode
        let current_mode: TransportMode = self.choose_initial_norm_and_habit(
            &subculture, self.subculture_connectivity, suggestibility,
            commute_length, &neighbourhood
        );
        let norm = current_mode;
        let last_mode = current_mode;

        // Create and return the agent
        Agent {
            subculture,
            neighbourhood,
            commute_length,
            weather_sensitivity,
            consistency,
            suggestibility,
            social_connectivity: self.social_connectivity,
            subculture_connectivity: self.subculture_connectivity,
            neighbourhood_connectivity: self.neighbourhood_connectivity,
            average_weight: 2.0 / (self.days_in_habit_average as f32 + 1.0),
            habit: hashmap!{current_mode => 1.0f32},
            current_mode,
            last_mode,
            norm,
            social_network: Vec::new(),
            neighbours: Vec::new(),
        }
    }

    /// Choose a random subculture, equal chance of each
    fn choose_subculture(&self) -> Arc<Subculture> {
        let mut weighted: Vec<distributions::Weighted<Arc<Subculture>>> = self.scenario.subcultures
            .iter()
            .map(|s: &Arc<Subculture>| distributions::Weighted {weight: 1, item: Arc::clone(s)})
            .collect();
        let weighted_choice = distributions::WeightedChoice::new(&mut weighted);
        weighted_choice.sample(&mut thread_rng())
    }

    /// Choose a random neighbourhood, equal chance of each
    fn choose_neighbourhood(&self) -> Arc<Neighbourhood> {
        let mut weighted: Vec<distributions::Weighted<Arc<Neighbourhood>>> = self.scenario.neighbourhoods
            .iter()
            .map(|s: &Arc<Neighbourhood>| distributions::Weighted {weight: 1, item: Arc::clone(s)})
            .collect();
        let weighted_choice = distributions::WeightedChoice::new(&mut weighted);
        weighted_choice.sample(&mut thread_rng())
    }

    /// Choose a random JourneyType, equal chance of each
    fn choose_journey_type(&self) -> JourneyType {
        let x = rand::random::<f32>();
        if x <= 0.33 {
            JourneyType::LocalCommute
        } else if x <= 0.66 {
            JourneyType::CityCommute
        } else {
            JourneyType::DistantCommute
        }
    }

    // Choose an initial norm an habit
    // TODO: Rethink this function
    fn choose_initial_norm_and_habit(&self,
                                     subculture: &Arc<Subculture>,
                                     subculture_connectivity: f32,
                                     suggestibility: f32,
                                     commute_length: JourneyType,
                                     neighbourhood: &Arc<Neighbourhood>
    ) -> TransportMode {
        let subculture_weight = subculture_connectivity;
        let mut initial_budget: HashMap<TransportMode, f32> = subculture
            .desirability
            .iter()
            .map(|(&k, v)|(k, v * subculture_weight))
            .collect();

        // Find the key-value-pair in initial_budger with the highest value, and store the value
        let max: f32 = initial_budget
            .iter()
            .fold((TransportMode::Walk, -0.1),
                  |(k0, v0): (TransportMode, f32), (&k1, &v1): (&TransportMode, &f32)| if v1 > v0 { (k1, v1) } else { (k0, v0) })
            .1;

        // Make it so the the max mode has a budget of 1, therefore at least one mode is always possible
        initial_budget = initial_budget
            .into_iter()
            .map(|(k, v)| (k, v / max))
            .collect();

        let commute_length_cost = commute_length.cost();

        // Take the supportiveness for each mode, away from 1, so a lower supportiveness = higher cost
        let neighbourhood_vals: HashMap<TransportMode, f32> = neighbourhood
            .supportiveness
            .iter()
            .map(|(&k, v)| (k, 1.0f32 - v))
            .collect();

        let values_to_average: Vec<&HashMap<TransportMode, f32>> =
            vec![&neighbourhood_vals,
                 &commute_length_cost];

        let mut initial_cost: HashMap<TransportMode, f32> = values_to_average
            .iter()
            .fold(HashMap::new(), |acc, x|
                union_of(&acc, x, |v1, v2| v1 + v2)
            )
            .into_iter()
            .map(|(k, v)| (k, v / (values_to_average.len() as f32)))
            .collect();

        // Find the minimum key-value pair of initial_cost and get its value
        let min = initial_cost
            .iter()
            .fold((TransportMode::Car, 9.99999999f32),
                  |(k0, v0): (TransportMode, f32), (&k1, &v1): (&TransportMode, &f32)| if v1 < v0 { (k1, v1) } else { (k0, v0) })
            .1;

        // If the minimum is greater than 1
        // Divide all the data by the minimum so that at least one mode has a cost of 1, so that a
        // mode is always possible
        if min > 1.0 {
            initial_cost = initial_cost
                .into_iter()
                .map(|(k, v)| (k, v / min))
                .collect();
        };

        // Filter out values where the budget is not greater than or equal to the cost
        // Calculate the difference between the budget and the cost
        // Get the maximum key-value pair (by value)
        // Set the current_mode equal to the key
        initial_budget
            .iter()
            .filter_map(|(&k, &v)| {
                let cost_val: f32 = *initial_cost.get(&k).unwrap_or(&9999.0f32);
                if v >= cost_val {
                    Some((k, v - cost_val))
                } else {
                    None
                }
            })
            .fold((TransportMode::Walk, -0.1),
                  |(k0, v0): (TransportMode, f32), (k1, v1): (TransportMode, f32)| if v1 > v0 { (k1, v1) } else { (k0, v0) })
            .0

        // if initial_mode == TransportMode::Cycle && commute_length == JourneyType::Cycle {
        //     println("{}", initial_cost.get(&TransportMode::Cycle).unwrap());
        // }
    }

    /// Link agents to a social network
    /// https://en.wikipedia.org/wiki/Barab%C3%A1si%E2%80%93Albert_model
    /// agents: a slice of agents
    /// n: the minimum number of links
    fn link_agents_to_neighbours(&self, agents: &[Rc<RefCell<Agent>>], n: u32) {
        // Create an empty Vec to store linked agents in
        let mut linked_agents: Vec<Rc<RefCell<Agent>>> = Vec::new();

        // For each rc pointer in agents
        for rc in agents.iter() {
            // if there are not yet n agents linked
            if linked_agents.len() < n as usize {
                // link the new agent, to all others
                for linked_agent in linked_agents.iter() {
                    rc.borrow_mut().neighbours.push(linked_agent.clone());
                    linked_agent.borrow_mut().neighbours.push(rc.clone())
                }
            } else {
                // Otherwise, link rc to n linked_agents, using preferential attachment
                let mut weighted: Vec<distributions::Weighted<Rc<RefCell<Agent>>>> = linked_agents
                    .iter()
                    .map(|a| distributions::Weighted {weight: a.borrow().neighbours.len() as u32, item: a.clone()})
                    .collect();
                let weighted_choice = distributions::WeightedChoice::new(&mut weighted);
                for _ in 0..n {
                    let friend = &mut weighted_choice.sample(&mut thread_rng());
                    friend.borrow_mut().neighbours.push(rc.clone());
                    rc.borrow_mut().neighbours.push(friend.clone());
                }
            }
            linked_agents.push(rc.clone());
            println!("[{}] Neighbour Network Size: {}", self.id, linked_agents.len());
        }
    }
}

/// Generate the header for the csv file
/// scenario: The scenario for this simulation
/// Returns: The header for the csv file
fn generate_csv_header(scenario: &Scenario) -> String {
    let subculture_ids: Vec<String> = scenario
        .subcultures
        .iter()
        .map(|subculture| subculture.id.clone())
        .collect();

    let neighbourhood_ids: Vec<String> = scenario
        .neighbourhoods
        .iter()
        .map(|neighbourhood| neighbourhood.id.clone())
        .collect();

    format!(
        "Day,Rain,ActiveMode,ActiveNorm,ActiveModeCounterToInactiveNorm,InactiveModeCounterToActiveNorm,LocalCommute,CityCommute,DistantCommute,{},{}\n",
        subculture_ids.join(","),
        neighbourhood_ids.join(",")
    )
}

fn generate_csv_output(day: u32, weather: &Weather, scenario: &Scenario,agents: &[Rc<RefCell<Agent>>]) -> String {
    let rain = if weather == &Weather::Good { 0 } else { 1 };

    let active_mode = statistics::count_active_mode(agents);
    let active_norm = statistics::count_active_norm(agents);
    let active_mode_counter_to_inactive_norm =
        statistics::count_active_mode_counter_to_inactive_norm(agents);

    let inactive_mode_counter_to_active_norm =
        statistics::count_inactive_mode_counter_to_active_norm(agents);

    let active_mode_by_commute_length = statistics::count_active_mode_by_commute_length(agents);
    let local_commute = active_mode_by_commute_length.get(&JourneyType::LocalCommute).unwrap();
    let city_commute = active_mode_by_commute_length.get(&JourneyType::CityCommute).unwrap();
    let distant_commute = active_mode_by_commute_length.get(&JourneyType::DistantCommute).unwrap();

    let active_mode_by_subculture =
        statistics::count_active_mode_by_subculture(agents);

    let active_mode_by_subculture_in_correct_order: Vec<String> = scenario
        .subcultures
        .iter()
        .map(|subculture| active_mode_by_subculture.get(subculture).unwrap_or(&0usize).to_string())
        .collect();

    let active_mode_by_neighbourhood =
        statistics::count_active_mode_by_neighbourhood(agents);

    let active_mode_by_neighbourhood_in_correct_order: Vec<String> = scenario
        .neighbourhoods
        .iter()
        .map(|neighbourhood| active_mode_by_neighbourhood.get(neighbourhood).unwrap_or(&0usize).to_string())
        .collect();

    format!(
        "{},{},{},{},{},{},{},{},{},{},{}\n",
        day,
        rain,
        active_mode,
        active_norm,
        active_mode_counter_to_inactive_norm,
        inactive_mode_counter_to_active_norm,
        local_commute,
        city_commute,
        distant_commute,
        active_mode_by_subculture_in_correct_order.join(","),
        active_mode_by_neighbourhood_in_correct_order.join(",")
    )
}

/// Link agents to a predefined social network
/// agents: a slice of agents
/// network: A map from agent id, to a vector of friend ids
fn link_agents_from_predefined_network(
    agents: &mut [Rc<RefCell<Agent>>], network: HashMap<u32, Vec<u32>>)
{
    network
        .iter()
        .for_each(|(&k, v)| {
            let mut friends: Vec<Rc<RefCell<Agent>>> = v
                .iter()
                .map(|&id| agents[id as usize].clone())
                .collect();
            agents[k as usize].borrow_mut().social_network.append(&mut friends);
        });
    agents.iter()
        .for_each(|a| println!("{}", a.borrow().social_network.len()))
}


/// Calculate whether a given day is a weekday
/// day: The day number
/// Returns: true iff day is a weekday
fn weekday(day: u32) -> bool {
    day % 7 < 5
}

/// Gets a random number from a normal distribution
/// mean: The mean of the normal distribution
/// sd: The standard deviation of the normal distributions
/// Returns: A random float from N(mean, sd)
fn random_normal(mean: f32, sd: f32) -> f32 {
    rand::distributions::Normal::new(mean as f64, sd as f64)
        .sample(&mut rand::thread_rng()) as f32
}

/// Count the statistics of the agents
/// Returns: A tuple (Number of active commutes, Number of active commutes counter to their (inactive) norm,
///     Number of inactive commutes counter to their norm, Number of agents who's norm is active)
fn count_stats(residents: &Vec<Rc<RefCell<Agent>>>) -> (usize, usize, usize, usize) {
    (residents.iter().filter(|&a| a.borrow().current_mode == TransportMode::Walk || a.borrow().current_mode == TransportMode::Cycle).count(),
     residents.iter().filter(|&a| (a.borrow().current_mode == TransportMode::Walk || a.borrow().current_mode == TransportMode::Cycle) && (a.borrow().norm != TransportMode::Walk && a.borrow().norm != TransportMode::Cycle)).count(),
     residents.iter().filter(|&a| (a.borrow().current_mode != TransportMode::Walk && a.borrow().current_mode != TransportMode::Cycle) && (a.borrow().norm == TransportMode::Walk || a.borrow().norm == TransportMode::Cycle)).count(),
     residents.iter().filter(|&a| a.borrow().norm == TransportMode::Walk || a.borrow().norm == TransportMode::Cycle).count()
    )
}
