use config::Configuration;
use holochain_cas_implementations::{
    cas::file::FilesystemStorage, eav::file::EavFileStorage, path::create_path_if_not_exists,
};
use holochain_core::context::Context;
use holochain_core_types::{error::HolochainError, json::JsonString};
use holochain_dna::Dna;
use Holochain;

use holochain_core::{logger::Logger, persister::SimplePersister};
use holochain_core_types::entry::agent::Agent;
use std::{
    collections::HashMap,
    convert::TryFrom,
    fs::File,
    io::prelude::*,
    sync::{Arc, Mutex, RwLock},
};

use boolinator::*;

pub struct Container {
    pub instances: HashMap<String, Holochain>,
    dna_loader: DnaLoader,
}

type DnaLoader = Arc<Box<FnMut(&String) -> Result<Dna, HolochainError> + Send>>;

impl Container {
    pub fn new() -> Self {
        Container {
            instances: HashMap::new(),
            dna_loader: Arc::new(Box::new(Self::load_dna)),
        }
    }

    pub fn start_all(&mut self) {
        let _ = self.instances.iter_mut().for_each(|(id, hc)| {
            println!("Starting instance \"{}\"...", id);
            match hc.start() {
                Ok(()) => println!("ok"),
                Err(err) => println!("Error: {}", err),
            }
        });
    }

    pub fn stop_all(&mut self) {
        let _ = self.instances.iter_mut().for_each(|(id, hc)| {
            println!("Stopping instance \"{}\"...", id);
            match hc.stop() {
                Ok(()) => println!("ok"),
                Err(err) => println!("Error: {}", err),
            }
        });
    }

    pub fn shutdown(&mut self) {
        self.stop_all();
        self.instances = HashMap::new();
    }

    pub fn load_config(&mut self, config: &Configuration) -> Result<(), String> {
        let _ = config.check_consistency()?;
        self.shutdown();
        let id_instance_pairs = config
            .instance_ids()
            .clone()
            .into_iter()
            .map(|id| {
                (
                    id.clone(),
                    instantiate_from_config(&id, config, &mut self.dna_loader),
                )
            })
            .collect::<Vec<_>>();

        id_instance_pairs
            .into_iter()
            .for_each(|(id, maybe_holochain)| match maybe_holochain {
                Ok(holochain) => {
                    self.instances.insert(id.clone(), holochain);
                }
                Err(error) => {
                    println!(
                        "Error while trying to create instance \"{}\": {}",
                        id, error
                    );
                }
            });
        Ok(())
    }

    fn load_dna(file: &String) -> Result<Dna, HolochainError> {
        let mut f = File::open(file)?;
        let mut contents = String::new();
        f.read_to_string(&mut contents)?;
        Dna::try_from(JsonString::from(contents))
    }
}

impl<'a> TryFrom<&'a Configuration> for Container {
    type Error = HolochainError;
    fn try_from(config: &'a Configuration) -> Result<Self, Self::Error> {
        let mut container = Container::new();
        container
            .load_config(config)
            .map_err(|string| HolochainError::ConfigError(string))?;
        Ok(container)
    }
}

fn instantiate_from_config(
    id: &String,
    config: &Configuration,
    dna_loader: &mut DnaLoader,
) -> Result<Holochain, String> {
    let _ = config.check_consistency()?;

    config
        .instance_by_id(&id)
        .ok_or(String::from("Instance not found in config"))
        .and_then(|instance_config| {
            let agent_config = config.agent_by_id(&instance_config.agent).unwrap();
            let dna_config = config.dna_by_id(&instance_config.dna).unwrap();
            let dna = Arc::get_mut(dna_loader).unwrap()(&dna_config.file).map_err(|_| {
                HolochainError::ConfigError(format!(
                    "Could not load DNA file \"{}\"",
                    dna_config.file
                ))
            })?;

            (instance_config.storage.storage_type == "file"
                && instance_config.storage.path.is_some())
                .ok_or(String::from("Only file storage supported currently"))?;

            let context = create_context(&agent_config.id, &instance_config.storage.path.unwrap())
                .map_err(|hc_err| format!("Error creating context: {}", hc_err.to_string()))?;

            Holochain::new(dna, Arc::new(context)).map_err(|hc_err| hc_err.to_string())
        })
}

#[derive(Clone, Debug)]
struct NullLogger {}

impl Logger for NullLogger {
    fn log(&mut self, _msg: String) {}
}

fn create_context(_: &String, path: &String) -> Result<Context, HolochainError> {
    let agent = Agent::from("c_bob".to_string());
    let cas_path = format!("{}/cas", path);
    let eav_path = format!("{}/eav", path);
    let agent_path = format!("{}/state", path);
    create_path_if_not_exists(&cas_path)?;
    create_path_if_not_exists(&eav_path)?;
    Context::new(
        agent,
        Arc::new(Mutex::new(NullLogger {})),
        Arc::new(Mutex::new(SimplePersister::new(agent_path))),
        Arc::new(RwLock::new(FilesystemStorage::new(&cas_path)?)),
        Arc::new(RwLock::new(EavFileStorage::new(eav_path)?)),
    )
}

mod tests {
    use super::*;
    use config::load_configuration;

    fn test_dna_loader() -> DnaLoader {
        let loader = Box::new(|_path: &String| Ok(Dna::new()))
            as Box<FnMut(&String) -> Result<Dna, HolochainError> + Send>;
        Arc::new(loader)
    }

    //#[test]
    // TODO
    // Deactivating this test because tests running in parallel creating Holochain instances
    // currently fail with:
    // "Error creating context: Failed to create actor in system: Failed to create actor.
    // Cause: An actor at the same path already exists"
    // This needs to be fixed in another PR.
    fn test_instantiate_from_config() {
        let toml = r#"
    [[agents]]
    id = "test agent"
    name = "Holo Tester"
    key_file = "holo_tester.key"

    [[dnas]]
    id = "app spec rust"
    file = "app-spec-rust.hcpkg"
    hash = "Qm328wyq38924y"

    [[instances]]
    id = "app spec instance"
    dna = "app spec rust"
    agent = "test agent"
    [instances.logger]
    type = "simple"
    file = "app_spec.log"
    [instances.storage]
    type = "file"
    path = "tmp-storage"

    "#;
        let config = load_configuration::<Configuration>(toml).unwrap();
        let maybe_holochain = instantiate_from_config(
            &"app spec instance".to_string(),
            &config,
            &mut test_dna_loader(),
        );

        assert_eq!(maybe_holochain.err(), None);
    }

    #[test]
    fn test_container_load_config() {
        let toml = r#"
    [[agents]]
    id = "test agent"
    name = "Holo Tester"
    key_file = "holo_tester.key"

    [[dnas]]
    id = "app spec rust"
    file = "app-spec-rust.hcpkg"
    hash = "Qm328wyq38924y"

    [[instances]]
    id = "app spec instance"
    dna = "app spec rust"
    agent = "test agent"
    [instances.logger]
    type = "simple"
    file = "app_spec.log"
    [instances.storage]
    type = "file"
    path = "tmp-storage"

    "#;
        let config = load_configuration::<Configuration>(toml).unwrap();

        let mut container = Container {
            instances: HashMap::new(),
            dna_loader: test_dna_loader(),
        };

        assert!(container.load_config(&config).is_ok());
        assert_eq!(container.instances.len(), 1);
    }
}
