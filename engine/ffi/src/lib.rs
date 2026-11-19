uniffi::setup_scaffolding!();
mod automation;
#[cfg(test)]
mod catalog_input_tests;
mod catalog_updates;
mod downloads;
mod group_network_permissions;
mod local_packages;
mod network_permissions;
mod outbound_permissions;
mod storage_usage;
mod transfers;
mod worker_egress;
pub use downloads::{InstallControl, InstallStatus};
pub use local_packages::LocalPackageTransfer;
pub use transfers::{
    BackupRestoreTransfer, DataReadTransfer, DataTransferControl, DataWriteTransfer,
};

/// Durable data operations. The future service must own this object; all methods
/// block and must run off the UI thread. Runtime lifecycle is a separate layer.
#[derive(uniffi::Object)]
pub struct EngineStorage {
    session: std::sync::Mutex<wasmdroid_engine::session::EngineSession>,
    review_issuer: std::sync::Arc<()>,
}

impl From<wasmdroid_engine::EngineError> for BridgeError {
    fn from(error: wasmdroid_engine::EngineError) -> Self {
        Self::Operation {
            reason: error.to_string(),
        }
    }
}
impl From<serde_json::Error> for BridgeError {
    fn from(error: serde_json::Error) -> Self {
        Self::Operation {
            reason: error.to_string(),
        }
    }
}

#[uniffi::export]
impl EngineStorage {
    /// Owns only streamed bytes, never a host path or an executable file fd.
    /// Importing does not create/start a service. Invoke off the main thread.
    pub fn begin_package_import(
        &self,
        expected_bytes: Option<u64>,
        max_new_bytes: u64,
        max_store_bytes: Option<u64>,
        control: std::sync::Arc<InstallControl>,
    ) -> Result<std::sync::Arc<LocalPackageTransfer>, BridgeError> {
        #[cfg(feature = "runtime")]
        {
            let connection = self
                .session
                .lock()
                .map_err(|_| storage_lock_error())?
                .connection()?;
            let transfer = wasmdroid_engine::local_import::LocalImport::begin(
                connection,
                wasmdroid_engine::store::InstallBudget {
                    max_new_bytes,
                    max_store_bytes,
                },
                expected_bytes,
                control.inner.clone(),
            )?;
            Ok(std::sync::Arc::new(LocalPackageTransfer {
                inner: std::sync::Mutex::new(transfer),
            }))
        }
        #[cfg(not(feature = "runtime"))]
        {
            let _ = (expected_bytes, max_new_bytes, max_store_bytes, control);
            Err(BridgeError::Operation {
                reason: "local package inspection requires the runtime driver".into(),
            })
        }
    }
    #[uniffi::constructor]
    pub fn open(data_dir: String) -> Result<std::sync::Arc<Self>, BridgeError> {
        let session = wasmdroid_engine::session::EngineSession::open(data_dir)?;
        wasmdroid_engine::volumes::VolumeManager::open(&session)?;
        Ok(std::sync::Arc::new(Self {
            session: std::sync::Mutex::new(session),
            review_issuer: std::sync::Arc::new(()),
        }))
    }
    pub fn list_volumes(&self) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| BridgeError::Operation {
            reason: "Engine storage lock failed".into(),
        })?;
        Ok(serde_json::to_string(&session.state().volumes()?)?)
    }
    /// Pure installed-package planning. Never downloads, creates volumes or starts
    /// guests. The returned plan contains private environment/argument values.
    pub fn preview_stack(&self, id: String, document: String) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        stack_json(&wasmdroid_engine::stacks::StackManager::new(&session).preview(&id, &document)?)
    }
    /// Inventory only. Download and normalized review remain explicit actions.
    pub fn stack_package_requirements(&self, document: String) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        stack_json(&wasmdroid_engine::stacks::package_requirements(
            &document,
            session.state(),
        )?)
    }
    /// Create durable definition/membership only. Runtime stack startup/routing
    /// belongs to the common platform controller and is a separate operation.
    pub fn create_stack(&self, id: String, document: String) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        stack_json(&wasmdroid_engine::stacks::StackManager::new(&session).create(&id, &document)?)
    }
    /// Publish exactly the reviewed normalized plan, rejecting changed artifacts.
    pub fn create_stack_from_preview(
        &self,
        id: String,
        preview: String,
    ) -> Result<String, BridgeError> {
        if preview.len() > 96 * 1024 {
            return Err(BridgeError::Operation {
                reason: "Stack preview exceeds IPC budget".into(),
            });
        }
        let plan: wasmdroid_engine::stacks::StackPlan =
            serde_json::from_str(&preview).map_err(|_| BridgeError::Operation {
                reason: "Invalid stack preview".into(),
            })?;
        if plan.id != id {
            return Err(BridgeError::Operation {
                reason: "Stack preview identity changed".into(),
            });
        }
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        stack_json(&wasmdroid_engine::stacks::StackManager::new(&session).create_plan(&plan)?)
    }
    pub fn inspect_stack(&self, id: String) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        stack_json(&session.state().stack(&id)?)
    }
    /// Private lossless Compose source from current members/restored datasets.
    pub fn stack_edit_source(&self, id: String) -> Result<String, BridgeError> {
        let handle = self
            .session
            .lock()
            .map_err(|_| storage_lock_error())?
            .connection_handle();
        let session = handle.connect()?;
        stack_json(&wasmdroid_engine::stacks::StackManager::new(&session).edit_source(&id)?)
    }
    pub fn preview_stack_update(
        &self,
        id: String,
        version: u64,
        document: String,
    ) -> Result<String, BridgeError> {
        let handle = self
            .session
            .lock()
            .map_err(|_| storage_lock_error())?
            .connection_handle();
        let session = handle.connect()?;
        stack_json(
            &wasmdroid_engine::stacks::StackManager::new(&session)
                .preview_update(&id, version, &document)?,
        )
    }
    /// Coordinator admission uses native-validated actual identities, never UI
    /// labels or a Kotlin reparse of a private/untrusted normalized plan.
    pub fn validate_stack_update_preview(
        &self,
        id: String,
        preview: String,
    ) -> Result<String, BridgeError> {
        let preview = read_stack_update(&id, &preview)?;
        let handle = self
            .session
            .lock()
            .map_err(|_| storage_lock_error())?
            .connection_handle();
        let session = handle.connect()?;
        preview.validate_current(session.state())?;
        stack_json(&preview.members)
    }
    /// The common controller must fence every current worker/data owner first.
    /// SQLite admission/preparation runs after releasing the shared facade lock.
    pub fn update_stack_from_preview(
        &self,
        id: String,
        preview: String,
    ) -> Result<String, BridgeError> {
        let preview = read_stack_update(&id, &preview)?;
        let handle = self
            .session
            .lock()
            .map_err(|_| storage_lock_error())?
            .connection_handle();
        let session = handle.connect()?;
        stack_json(&wasmdroid_engine::stacks::StackManager::new(&session).update_plan(&preview)?)
    }
    pub fn stack_members(&self, id: String) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        stack_json(&session.state().stack_members(&id)?)
    }
    pub fn stack_summary(&self, id: String) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        stack_json(&session.state().stack_summary(&id)?)
    }
    pub fn instance_stack(&self, id: String) -> Result<Option<String>, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        Ok(session.state().stack_for_instance(&id)?)
    }
    pub fn stack_plan(&self, id: String) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        let record = session.state().stack(&id)?;
        // Common startup consumes this plan before reserving any worker. An
        // explicit reimport must not silently change an existing group's pins.
        record.plan.validate(session.state())?;
        stack_json(&serde_json::json!({"id":record.id,"version":record.version,"plan":record.plan}))
    }
    pub fn retry_stack_member(
        &self,
        id: String,
        stack_version: u64,
        service: String,
        instance_version: u64,
        new_id: String,
    ) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        stack_json(&session.state().retry_stack_member(
            &id,
            stack_version,
            &service,
            instance_version,
            &new_id,
        )?)
    }
    /// Private coordinator API. The returned keys are host IPC only.
    pub fn stack_network_configs(
        &self,
        id: String,
        workers: String,
        previous: String,
        revision: u64,
    ) -> Result<String, BridgeError> {
        if workers.len() > 16 * 1024 || previous.len() > 128 * 1024 {
            return Err(private_network_error());
        }
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        let workers = serde_json::from_str(&workers).map_err(|_| private_network_error())?;
        let previous = serde_json::from_str(&previous).map_err(|_| private_network_error())?;
        stack_json(&wasmdroid_engine::network::grants::topology(
            session.state(),
            &id,
            &workers,
            &previous,
            revision,
        )?)
    }
    pub fn list_stacks(&self, after: Option<String>, limit: u32) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        Ok(serde_json::to_string(
            &session.state().stack_summaries(after.as_deref(), limit)?,
        )?)
    }
    pub fn remove_stack(&self, id: String, version: u64) -> Result<(), BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        Ok(session.state().remove_stack(&id, version)?)
    }
    pub fn create_volume(&self, name: String) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| BridgeError::Operation {
            reason: "Engine storage lock failed".into(),
        })?;
        let manager = wasmdroid_engine::volumes::VolumeManager::open(&session)?;
        Ok(serde_json::to_string(
            &manager.create(&name, Default::default())?,
        )?)
    }
    /// Explicit new dataset creation never reuses or replaces existing data.
    pub fn create_new_volume(&self, name: String) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        let manager = wasmdroid_engine::volumes::VolumeManager::open(&session)?;
        Ok(serde_json::to_string(
            &manager.create_new(&name, Default::default())?,
        )?)
    }
    pub fn volume_entries(&self, name: String, path: String) -> Result<String, BridgeError> {
        let session = self
            .session
            .lock()
            .map_err(|_| storage_lock_error())?
            .connection()?;
        let directory =
            wasmdroid_engine::volumes::VolumeManager::open(&session)?.lease(&name, true)?;
        Ok(serde_json::to_string(&wasmdroid_engine::data::list(
            &directory, &path,
        )?)?)
    }
    pub fn create_data_directory(&self, name: String, path: String) -> Result<(), BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        let directory =
            wasmdroid_engine::volumes::VolumeManager::open(&session)?.exclusive(&name)?;
        let path = wasmdroid_engine::data::file_path(&path)?;
        wasmdroid_engine::data::ensure_parent(&directory, &path)?;
        directory
            .create_dir(path)
            .map_err(wasmdroid_engine::EngineError::from)?;
        Ok(())
    }
    pub fn remove_data_entry(
        &self,
        name: String,
        path: String,
        directory: bool,
    ) -> Result<(), BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        let data = wasmdroid_engine::volumes::VolumeManager::open(&session)?.exclusive(&name)?;
        let path = wasmdroid_engine::data::file_path(&path)?;
        // Only explicitly selected files or empty folders, never recursive user
        // data deletion through a path supplied by a document provider.
        if directory {
            data.remove_dir(path)
        } else {
            data.remove_file(path)
        }
        .map_err(wasmdroid_engine::EngineError::from)?;
        Ok(())
    }
    pub fn begin_data_import(
        &self,
        name: String,
        path: String,
        overwrite: bool,
        expected_bytes: Option<u64>,
        max_bytes: u64,
        control: std::sync::Arc<DataTransferControl>,
    ) -> Result<std::sync::Arc<DataWriteTransfer>, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        let inner = wasmdroid_engine::data::DataImport::begin(
            &session,
            &name,
            &path,
            overwrite,
            expected_bytes,
            max_bytes,
            control.inner.clone(),
        )?;
        Ok(std::sync::Arc::new(DataWriteTransfer {
            inner: std::sync::Mutex::new(inner),
        }))
    }
    pub fn begin_data_export(
        &self,
        name: String,
        path: String,
    ) -> Result<std::sync::Arc<DataReadTransfer>, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        let inner = wasmdroid_engine::data::DataReader::file(&session, &name, &path)?;
        Ok(std::sync::Arc::new(DataReadTransfer {
            inner: std::sync::Mutex::new(inner),
        }))
    }
    pub fn begin_volume_backup(
        &self,
        name: String,
        max_bytes: u64,
        control: std::sync::Arc<DataTransferControl>,
    ) -> Result<std::sync::Arc<DataReadTransfer>, BridgeError> {
        let session = self
            .session
            .lock()
            .map_err(|_| storage_lock_error())?
            .connection()?;
        let inner =
            wasmdroid_engine::backup::snapshot(&session, &name, max_bytes, control.inner.clone())?;
        Ok(std::sync::Arc::new(DataReadTransfer {
            inner: std::sync::Mutex::new(inner),
        }))
    }
    pub fn begin_volume_restore(
        &self,
        new_name: String,
        max_archive_bytes: u64,
        max_unpacked_bytes: u64,
        control: std::sync::Arc<DataTransferControl>,
    ) -> Result<std::sync::Arc<BackupRestoreTransfer>, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        let inner = wasmdroid_engine::backup::RestoreInput::begin(
            &session,
            &new_name,
            max_archive_bytes,
            max_unpacked_bytes,
            control.inner.clone(),
        )?;
        Ok(std::sync::Arc::new(BackupRestoreTransfer {
            inner: std::sync::Mutex::new(inner),
        }))
    }
    pub fn attach_restored_volume(
        &self,
        id: String,
        version: u64,
        old_volume: String,
        new_volume: String,
    ) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        Ok(serde_json::to_string(
            &session
                .state()
                .replace_instance_volume(&id, version, &old_volume, &new_volume)?,
        )?)
    }
    pub fn remove_volume(&self, name: String) -> Result<(), BridgeError> {
        let session = self.session.lock().map_err(|_| BridgeError::Operation {
            reason: "Engine storage lock failed".into(),
        })?;
        wasmdroid_engine::volumes::VolumeManager::open(&session)?.remove(&name)?;
        Ok(())
    }
    pub fn list_instances(&self, endpoint_id: String) -> Result<String, BridgeError> {
        if endpoint_id != "local" {
            return Err(
                wasmdroid_engine::EngineError::Unsupported("remote endpoint".into()).into(),
            );
        }
        let session = self.session.lock().map_err(|_| BridgeError::Operation {
            reason: "Engine storage lock failed".into(),
        })?;
        Ok(serde_json::to_string(
            &session.state().instances(&endpoint_id)?,
        )?)
    }
    pub fn events_after(&self, sequence: u64, limit: u32) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| BridgeError::Operation {
            reason: "Engine storage lock failed".into(),
        })?;
        Ok(serde_json::to_string(
            &session.state().events_after(sequence, limit)?,
        )?)
    }

    pub fn install_catalog_package(
        &self,
        package_json: String,
        observer: Option<Box<dyn ProbeObserver>>,
    ) -> Result<String, BridgeError> {
        self.install_catalog_package_controlled(
            package_json,
            1024 * 1024 * 1024,
            None,
            InstallControl::new(),
            observer,
        )
    }
    pub fn install_catalog_package_controlled(
        &self,
        package_json: String,
        max_new_bytes: u64,
        max_store_bytes: Option<u64>,
        control: std::sync::Arc<InstallControl>,
        observer: Option<Box<dyn ProbeObserver>>,
    ) -> Result<String, BridgeError> {
        let mut operation = control.inner.claim()?;
        (|| {
            use wasmdroid_engine::{
                installer::{InstallLimits, Installer},
                registry::wasmer::WasmerRegistry,
                state::StateDb,
                store::{ContentStore, InstallBudget},
            };
            control.inner.check()?;
            let package = catalog_package(&package_json)?;
            if !package.android_verified {
                return Err(wasmdroid_engine::EngineError::Unsupported(
                    "curated installation requires Android evidence".into(),
                )
                .into());
            }
            let (root, _owner) = {
                let session = self.session.lock().map_err(|_| storage_lock_error())?;
                (session.root().to_owned(), session.ownership())
            };
            let registry = WasmerRegistry::with_control(control.inner.clone())?;
            let store = ContentStore::open(root.join("store"))?;
            let state = StateDb::open(root.join("engine.sqlite"))?;
            let installer = Installer {
                registry: &registry,
                store: &store,
                state: &state,
                limits: InstallLimits::default(),
            };
            let budget = InstallBudget {
                max_new_bytes,
                max_store_bytes,
            };
            let mut progress = |progress| {
                if let Some(observer) = &observer
                    && let Ok(json) = serde_json::to_string(&progress)
                {
                    observer.on_phase(json);
                }
            };
            if let Some(recipe) = package.single_recipe() {
                let graph = installer.pull_in_operation(
                    &recipe.artifact_ref,
                    Some(&package.digest),
                    &mut operation,
                    budget,
                    &mut progress,
                )?;
                Ok(serde_json::to_string(&graph)?)
            } else {
                let roots: Vec<_> = package
                    .artifacts()
                    .into_iter()
                    .map(|artifact| (artifact.reference, artifact.digest))
                    .collect();
                let graphs = installer.pull_many_expected_in_operation(
                    &roots,
                    &mut operation,
                    budget,
                    &mut progress,
                )?;
                // Group preparation re-reads native state. No duplicated graph
                // arrays or caller-supplied graph metadata grant authority.
                Ok(serde_json::json!({"roots":graphs.iter().map(|graph| &graph.root).collect::<Vec<_>>()}).to_string())
            }
        })()
    }
    pub fn validate_catalog_application(&self, package_json: String) -> Result<(), BridgeError> {
        let package = catalog_package(&package_json)?;
        if !package.android_verified {
            return Err(wasmdroid_engine::EngineError::Unsupported(
                "curated installation requires Android evidence".into(),
            )
            .into());
        }
        if package.single_recipe().is_some() {
            let effective = wasmdroid_engine::catalog_install::effective_single_spec(&package)?;
            self.validate_service_spec(serde_json::to_string(&effective)?)
        } else {
            #[cfg(feature = "runtime")]
            {
                Ok(())
            }
            #[cfg(not(feature = "runtime"))]
            {
                Err(
                    wasmdroid_engine::EngineError::Unsupported("runtime is not included".into())
                        .into(),
                )
            }
        }
    }
    pub fn prepare_catalog_stack(
        &self,
        id: String,
        package_json: String,
    ) -> Result<String, BridgeError> {
        let package = catalog_package(&package_json)?;
        #[cfg(not(feature = "runtime"))]
        {
            let _ = (id, package);
            Err(wasmdroid_engine::EngineError::Unsupported("runtime is not included".into()).into())
        }
        #[cfg(feature = "runtime")]
        {
            let handle = self
                .session
                .lock()
                .map_err(|_| storage_lock_error())?
                .connection_handle();
            let session = handle.connect()?;
            stack_json(&wasmdroid_engine::catalog_install::prepare_stack(
                &session,
                &id,
                &package,
                &mut |spec| wasmdroid_wasmer_local::service_execution::validate_spec(spec),
            )?)
        }
    }
    pub fn retry_dead_instance(
        &self,
        id: String,
        version: u64,
        new_id: String,
    ) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        Ok(serde_json::to_string(
            &session.state().retry_dead_instance(&id, version, &new_id)?,
        )?)
    }
    pub fn list_images(&self) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        Ok(serde_json::to_string(&session.state().artifacts()?)?)
    }
    pub fn application_targets(
        &self,
        after: Option<String>,
        limit: u32,
    ) -> Result<String, BridgeError> {
        let handle = self
            .session
            .lock()
            .map_err(|_| storage_lock_error())?
            .connection_handle();
        let session = handle.connect()?;
        stack_json(&session.state().application_page(after.as_deref(), limit)?)
    }
    pub fn application_target(&self, kind: String, id: String) -> Result<String, BridgeError> {
        let kind: wasmdroid_engine::state::ApplicationTargetKind =
            serde_json::from_value(serde_json::Value::String(kind))?;
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        stack_json(&session.state().application_target(kind, &id)?)
    }
    pub fn application_control_epoch(&self) -> Result<u64, BridgeError> {
        Ok(self
            .session
            .lock()
            .map_err(|_| storage_lock_error())?
            .state()
            .application_control_epoch()?)
    }
    pub fn resolve_application_target(&self, target_json: String) -> Result<String, BridgeError> {
        if target_json.len() > 4096 {
            return Err(wasmdroid_engine::EngineError::Invalid(
                "application target exceeds input limit".into(),
            )
            .into());
        }
        let target: wasmdroid_engine::state::ApplicationTarget = serde_json::from_str(&target_json)
            .map_err(|_| BridgeError::Operation {
                reason: "Invalid application target".into(),
            })?;
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        stack_json(&session.state().resolve_application_target(&target)?)
    }
    pub fn remove_image(&self, reference: String) -> Result<String, BridgeError> {
        let session = self
            .session
            .lock()
            .map_err(|_| storage_lock_error())?
            .connection()?;
        let store = wasmdroid_engine::store::ContentStore::open(session.root().join("store"))?;
        let report = store.uninstall(
            session.state(),
            &wasmdroid_engine::api::ArtifactReference::parse(&reference)?,
        )?;
        Ok(serde_json::to_string(&report)?)
    }
    pub fn collect_unused_cache(&self) -> Result<String, BridgeError> {
        let session = self
            .session
            .lock()
            .map_err(|_| storage_lock_error())?
            .connection()?;
        let store = wasmdroid_engine::store::ContentStore::open(session.root().join("store"))?;
        let downloads = store.collect_garbage(session.state())?;
        let compiled =
            wasmdroid_engine::compiled_cache::TrustedCodeCache::open(&session)?.clear()?;
        Ok(serde_json::json!({"downloads":downloads,"compiled":compiled}).to_string())
    }
    pub fn inspect_instance(&self, id: String) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        Ok(serde_json::to_string(&session.state().instance(&id)?)?)
    }
    pub fn instance_ports(&self, id: String) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        Ok(serde_json::to_string(
            &session.state().instance_ports(&id)?,
        )?)
    }
    /// Pure preflight; no volumes, instance metadata or guests are created.
    pub fn validate_service_spec(&self, spec_json: String) -> Result<(), BridgeError> {
        if spec_json.len() > 128 * 1024 {
            return Err(wasmdroid_engine::EngineError::Invalid(
                "service specification exceeds IPC limit".into(),
            )
            .into());
        }
        let spec: wasmdroid_engine::api::InstanceSpec = serde_json::from_str(&spec_json)?;
        #[cfg(feature = "runtime")]
        {
            #[cfg(target_os = "android")]
            wasmdroid_wasmer_local::service_execution::validate_spec_for_egress_configuration(
                &spec,
            )?;
            #[cfg(not(target_os = "android"))]
            wasmdroid_wasmer_local::service_execution::validate_spec(&spec)?;
            Ok(())
        }
        #[cfg(not(feature = "runtime"))]
        {
            let _ = spec;
            Err(wasmdroid_engine::EngineError::Unsupported("runtime is not included".into()).into())
        }
    }
    pub fn create_instance(&self, id: String, spec_json: String) -> Result<String, BridgeError> {
        if spec_json.len() > 128 * 1024 {
            return Err(wasmdroid_engine::EngineError::Invalid(
                "service specification exceeds IPC limit".into(),
            )
            .into());
        }
        let spec: wasmdroid_engine::api::InstanceSpec = serde_json::from_str(&spec_json)?;
        #[cfg(feature = "runtime")]
        wasmdroid_wasmer_local::service_execution::validate_spec(&spec)?;
        #[cfg(not(feature = "runtime"))]
        {
            let _ = (id, spec);
            Err(wasmdroid_engine::EngineError::Unsupported("runtime is not included".into()).into())
        }
        #[cfg(feature = "runtime")]
        {
            let session = self.session.lock().map_err(|_| storage_lock_error())?;
            Ok(serde_json::to_string(
                &session.state().create_instance(&id, &spec)?,
            )?)
        }
    }
    /// Common-owner metadata edit after observed worker death/idle fencing.
    /// Saving neither starts a guest nor creates/removes datasets.
    pub fn configure_instance(
        &self,
        id: String,
        version: u64,
        spec_json: String,
    ) -> Result<String, BridgeError> {
        if spec_json.len() > 128 * 1024 {
            return Err(wasmdroid_engine::EngineError::Invalid(
                "service specification exceeds IPC limit".into(),
            )
            .into());
        }
        let spec: wasmdroid_engine::api::InstanceSpec =
            serde_json::from_str(&spec_json).map_err(|_| {
                wasmdroid_engine::EngineError::Invalid("invalid service configuration".into())
            })?;
        #[cfg(feature = "runtime")]
        {
            #[cfg(target_os = "android")]
            wasmdroid_wasmer_local::service_execution::validate_spec_for_egress_configuration(
                &spec,
            )?;
            #[cfg(not(target_os = "android"))]
            wasmdroid_wasmer_local::service_execution::validate_spec(&spec)?;
            // SQLite admission can wait. Keep engine ownership while releasing
            // the shared facade mutex before opening/writing this connection.
            let (root, _ownership) = {
                let session = self.session.lock().map_err(|_| storage_lock_error())?;
                (session.root().to_owned(), session.ownership())
            };
            let state = wasmdroid_engine::state::StateDb::open(root.join("engine.sqlite"))?;
            #[cfg(target_os = "android")]
            let updated =
                state.configure_instance_preserving_network_permissions(&id, version, &spec)?;
            #[cfg(not(target_os = "android"))]
            let updated = state.configure_instance(&id, version, &spec)?;
            Ok(serde_json::to_string(&updated)?)
        }
        #[cfg(not(feature = "runtime"))]
        {
            let _ = (id, version, spec);
            Err(wasmdroid_engine::EngineError::Unsupported("runtime is not included".into()).into())
        }
    }
    pub fn mark_running(
        &self,
        id: String,
        version: u64,
        ports_json: String,
    ) -> Result<String, BridgeError> {
        let ports: Vec<wasmdroid_engine::api::PortBinding> = serde_json::from_str(&ports_json)?;
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        Ok(serde_json::to_string(
            &session.state().start_instance(&id, version, &ports)?,
        )?)
    }
    /// Only call after the owning worker's death has actually been observed.
    pub fn mark_finished(
        &self,
        id: String,
        version: u64,
        exit_code: Option<i32>,
        reason: Option<String>,
    ) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        let state = if exit_code.is_some() {
            wasmdroid_engine::api::InstanceState::Exited
        } else {
            wasmdroid_engine::api::InstanceState::Dead
        };
        Ok(serde_json::to_string(
            &session.state().transition_instance(
                &id,
                version,
                state,
                exit_code,
                reason.as_deref(),
            )?,
        )?)
    }
    pub fn mark_stopped(&self, id: String, version: u64) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        let previous = session.state().instance(&id)?;
        let state = if previous.state == wasmdroid_engine::api::InstanceState::Created {
            wasmdroid_engine::api::InstanceState::Dead
        } else {
            wasmdroid_engine::api::InstanceState::Exited
        };
        Ok(serde_json::to_string(
            &session.state().transition_instance(
                &id,
                version,
                state,
                None,
                Some("Stopped by owner after observed worker death; guest exit code unknown"),
            )?,
        )?)
    }
    pub fn remove_instance(&self, id: String, version: u64) -> Result<(), BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        Ok(session.state().remove_instance(&id, version)?)
    }
    pub fn save_output(
        &self,
        id: String,
        generation: String,
        output_json: String,
    ) -> Result<(), BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        session.state().instance(&id)?;
        Ok(
            wasmdroid_engine::logs::LogStore::open(session.root().join("logs"))?.store(
                &id,
                &generation,
                &output_json,
            )?,
        )
    }
    pub fn latest_output(&self, id: String) -> Result<Option<String>, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        session.state().instance(&id)?;
        Ok(wasmdroid_engine::logs::LogStore::open(session.root().join("logs"))?.latest(&id)?)
    }
    pub fn begin_log_run(&self, id: String, generation: String) -> Result<(), BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        session.state().instance(&id)?;
        Ok(wasmdroid_engine::logs::live::begin_run(
            &session,
            &id,
            &generation,
        )?)
    }
    pub fn read_logs(&self, id: String, query_json: String) -> Result<Option<String>, BridgeError> {
        if query_json.len() > 4096 {
            return Err(
                wasmdroid_engine::EngineError::Invalid("log query exceeds limit".into()).into(),
            );
        }
        let query = serde_json::from_str(&query_json)?;
        let session = self
            .session
            .lock()
            .map_err(|_| storage_lock_error())?
            .connection()?;
        session.state().instance(&id)?;
        wasmdroid_engine::logs::live::read(&session, &id, &query)?
            .map(|page| serde_json::to_string(&page).map_err(BridgeError::from))
            .transpose()
    }
    pub fn saved_terminal(
        &self,
        id: String,
        generation: String,
        start_row: u16,
    ) -> Result<Option<String>, BridgeError> {
        let session = self
            .session
            .lock()
            .map_err(|_| storage_lock_error())?
            .connection()?;
        session.state().instance(&id)?;
        Ok(wasmdroid_engine::logs::live::read_terminal(
            &session,
            &id,
            &generation,
            start_row,
        )?)
    }
    /// Ownership is transferred to ParcelFileDescriptor, then to the worker.
    pub fn duplicate_worker_owner(&self) -> Result<i32, BridgeError> {
        #[cfg(unix)]
        {
            use std::os::fd::IntoRawFd;
            let session = self.session.lock().map_err(|_| storage_lock_error())?;
            Ok(session.duplicate_owner_descriptor()?.into_raw_fd())
        }
        #[cfg(not(unix))]
        {
            Err(
                wasmdroid_engine::EngineError::Unsupported("worker descriptor platform".into())
                    .into(),
            )
        }
    }
    pub fn pull_image(
        &self,
        reference: String,
        observer: Option<Box<dyn ProbeObserver>>,
    ) -> Result<String, BridgeError> {
        self.pull_image_controlled(
            reference,
            1024 * 1024 * 1024,
            None,
            InstallControl::new(),
            observer,
        )
    }
    pub fn pull_image_controlled(
        &self,
        reference: String,
        max_new_bytes: u64,
        max_store_bytes: Option<u64>,
        control: std::sync::Arc<InstallControl>,
        observer: Option<Box<dyn ProbeObserver>>,
    ) -> Result<String, BridgeError> {
        let mut operation = control.inner.claim()?;
        (|| {
            use wasmdroid_engine::{
                api::ArtifactReference,
                installer::{InstallLimits, Installer},
                registry::wasmer::WasmerRegistry,
                state::StateDb,
                store::{ContentStore, InstallBudget},
            };
            control.inner.check()?;
            let reference = ArtifactReference::parse(&reference)?;
            let (root, _owner) = {
                let session = self.session.lock().map_err(|_| storage_lock_error())?;
                (session.root().to_owned(), session.ownership())
            };
            let registry = WasmerRegistry::with_control(control.inner.clone())?;
            let store = ContentStore::open(root.join("store"))?;
            let state = StateDb::open(root.join("engine.sqlite"))?;
            let graph = Installer {
                registry: &registry,
                store: &store,
                state: &state,
                limits: InstallLimits::default(),
            }
            .pull_in_operation(
                &reference,
                None,
                &mut operation,
                InstallBudget {
                    max_new_bytes,
                    max_store_bytes,
                },
                &mut |progress| {
                    if let Some(observer) = &observer
                        && let Ok(json) = serde_json::to_string(&progress)
                    {
                        observer.on_phase(json);
                    }
                },
            )?;
            Ok(serde_json::to_string(&graph)?)
        })()
    }
    pub fn search_registry(
        &self,
        query: String,
        cursor: Option<String>,
        exact_release: Option<String>,
    ) -> Result<String, BridgeError> {
        use wasmdroid_engine::registry::Registry;
        let registry = wasmdroid_engine::registry::wasmer::WasmerRegistry::new()?;
        let exact_release = exact_release
            .filter(|_| cursor.is_none())
            .and_then(|reference| wasmdroid_engine::api::ArtifactReference::parse(reference).ok());
        let limit = if exact_release.is_some() { 29 } else { 30 };
        let mut page = registry.search(&query, cursor.as_deref(), limit)?;
        // A best-effort exact lookup supplements the provider's text index.
        // Keep the original result and cursor if this optional lookup fails.
        if let Some(reference) = exact_release
            && !page.packages.iter().any(|row| row.reference == reference)
            && let Ok(Some(package)) = registry.find_public_version(&reference)
        {
            page.packages.push(package);
        }
        Ok(serde_json::to_string(&page)?)
    }
}
fn storage_lock_error() -> BridgeError {
    BridgeError::Operation {
        reason: "Engine storage lock failed".into(),
    }
}

fn catalog_package(json: &str) -> Result<wasmdroid_engine::catalog::CatalogPackage, BridgeError> {
    if json.len() > 128 * 1024 {
        return Err(wasmdroid_engine::EngineError::Invalid(
            "catalog entry exceeds IPC limit".into(),
        )
        .into());
    }
    let package: wasmdroid_engine::catalog::CatalogPackage =
        serde_json::from_str(json).map_err(|_| BridgeError::Operation {
            reason: "Invalid catalog entry".into(),
        })?;
    // Single-entry calls retain the legacy shape. The enclosing accepted
    // catalog supplies version/trust authority; media is the schema-3 feature
    // needing validation here before any prepare/install operation.
    let schema_version = if package.media.is_some() { 3 } else { 2 };
    let mut catalog = wasmdroid_engine::catalog::Catalog::parse(&serde_json::to_string(
        &wasmdroid_engine::catalog::Catalog {
            schema_version,
            packages: vec![package],
        },
    )?)?;
    Ok(catalog.packages.remove(0))
}
fn read_stack_update(
    id: &str,
    preview: &str,
) -> Result<wasmdroid_engine::stacks::StackUpdatePreview, BridgeError> {
    if preview.len() > 128 * 1024 {
        return Err(BridgeError::Operation {
            reason: "Stack update exceeds IPC budget".into(),
        });
    }
    let preview: wasmdroid_engine::stacks::StackUpdatePreview = serde_json::from_str(preview)
        .map_err(|_| BridgeError::Operation {
            reason: "Invalid stack update preview".into(),
        })?;
    if preview.plan.id != id {
        return Err(BridgeError::Operation {
            reason: "Stack update identity changed".into(),
        });
    }
    Ok(preview)
}
fn stack_json(value: &impl serde::Serialize) -> Result<String, BridgeError> {
    let json = serde_json::to_string(value)?;
    if json.len() > 256 * 1024 {
        return Err(BridgeError::Operation {
            reason: "Stack response exceeds IPC limit; inspect individual members".into(),
        });
    }
    Ok(json)
}
fn private_network_error() -> BridgeError {
    BridgeError::Operation {
        reason: "Invalid private network request".into(),
    }
}

/// Trusted private Android worker control. No guest or registry input can
/// provide this callback, and it must terminate only its own worker process.
#[uniffi::export(callback_interface)]
pub trait WorkerControl: Send + Sync {
    fn terminate_after(&self, grace_seconds: u64);
    fn terminate(&self);
}

/// Same-process private worker callback. The native listener retains the
/// descriptor while Java duplicates it and binds that socket to its captured
/// current Android Network. A guest/caller URL cannot supply this capability.
#[uniffi::export(callback_interface)]
pub trait LanSocketBinder: Send + Sync {
    fn bind_socket(&self, descriptor: i32, grant_epoch: u64, private_address: String) -> bool;
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LanControlScope {
    instance_id: String,
    generation: String,
    instance_port: u16,
    grant_epoch: u64,
    private_address: std::net::Ipv4Addr,
    ipv6_address: Option<std::net::Ipv6Addr>,
    ipv6_prefix_length: Option<u8>,
}
fn lan_control_error() -> BridgeError {
    BridgeError::Operation {
        reason: "Invalid private LAN control request".into(),
    }
}
fn lan_control_scope(json: &str) -> Result<wasmdroid_engine::network::lan::LanScope, BridgeError> {
    if json.len() > 2048 {
        return Err(lan_control_error());
    }
    let scope: LanControlScope = serde_json::from_str(json).map_err(|_| lan_control_error())?;
    if scope.grant_epoch > i64::MAX as u64 {
        return Err(lan_control_error());
    }
    use wasmdroid_engine::network::lan::{LanIpv6, LanScope};
    let ipv6 = match (scope.ipv6_address, scope.ipv6_prefix_length) {
        (Some(address), Some(prefix)) => {
            Some(LanIpv6::new(address, prefix).map_err(|_| lan_control_error())?)
        }
        (None, None) => None,
        _ => return Err(lan_control_error()),
    };
    LanScope::new(
        scope.instance_id,
        scope.generation,
        scope.instance_port,
        scope.grant_epoch,
        scope.private_address,
    )
    .map(|scope| scope.with_ipv6(ipv6))
    .map_err(|_| lan_control_error())
}

#[cfg(test)]
mod lan_control_tests {
    use super::lan_control_scope;

    #[test]
    fn private_lan_wire_retains_the_exact_scoped_endpoint_and_signed_epoch_boundary() {
        let value = serde_json::json!({"instance_id":"3","generation":"scope-generation","instance_port":8080,
            "grant_epoch":i64::MAX,"private_address":"192.168.1.24"});
        let scope = lan_control_scope(&value.to_string()).unwrap();
        assert_eq!(scope.instance_id(), "3");
        assert_eq!(scope.generation(), "scope-generation");
        assert_eq!(scope.guest_port(), 8080);
        assert_eq!(scope.interface_epoch(), i64::MAX as u64);
        assert_eq!(scope.private_address().octets(), [192, 168, 1, 24]);
    }
    #[test]
    fn malformed_lan_requests_are_rejected_without_coercion_dns_or_private_text_in_errors() {
        let valid = serde_json::json!({"instance_id":"3","generation":"scope-generation","instance_port":8080,
            "grant_epoch":1,"private_address":"192.168.1.24"});
        for (key, value) in [
            ("instance_id", serde_json::json!(3)),
            ("generation", serde_json::json!(4)),
            ("instance_port", serde_json::json!("8080")),
            ("grant_epoch", serde_json::json!(0)),
            ("grant_epoch", serde_json::json!(u64::MAX)),
            ("private_address", serde_json::json!("PRIVATE.example")),
            ("private_address", serde_json::json!("127.0.0.1")),
            ("PRIVATE_FIELD", serde_json::json!("PRIVATE_VALUE")),
        ] {
            let mut request = valid.clone();
            request[key] = value;
            let error = lan_control_scope(&request.to_string())
                .unwrap_err()
                .to_string();
            assert!(error.contains("Invalid private LAN control request"));
            assert!(!error.contains("PRIVATE"));
        }
        assert!(lan_control_scope(&"PRIVATE".repeat(2048)).is_err());
    }
}

#[cfg(feature = "runtime")]
struct PlatformControl(Box<dyn WorkerControl>);
#[cfg(feature = "runtime")]
impl wasmdroid_wasmer_local::service_driver::WorkerPlatform for PlatformControl {
    fn terminate_after(&self, grace_seconds: u64) {
        self.0.terminate_after(grace_seconds);
    }
    fn terminate(&self) {
        self.0.terminate();
    }
}

/// Worker-only driver facade. The coordinator must still observe Binder death
/// before releasing the group, even after native poll/wait reports an exit.
#[derive(uniffi::Object)]
pub struct ServiceRuntime {
    #[cfg(feature = "runtime")]
    inner: std::sync::Arc<wasmdroid_wasmer_local::service_driver::ServiceDriver>,
}
#[uniffi::export]
impl ServiceRuntime {
    #[uniffi::constructor]
    pub fn start(
        data_dir: String,
        owner_descriptor: i32,
        instance_id: String,
        generation: String,
        spec_json: String,
        worker_control: Box<dyn WorkerControl>,
        network_config: Option<String>,
    ) -> Result<std::sync::Arc<Self>, BridgeError> {
        #[cfg(unix)]
        {
            use std::os::fd::FromRawFd;
            if owner_descriptor < 0 {
                return Err(BridgeError::Operation {
                    reason: "invalid worker ownership descriptor".into(),
                });
            }
            // SAFETY: private Android worker transfers one owned detached fd.
            // This constructor consumes it on both success and error.
            let owner = unsafe { std::fs::File::from_raw_fd(owner_descriptor) };
            #[cfg(feature = "runtime")]
            {
                if network_config
                    .as_ref()
                    .is_some_and(|json| json.len() > 32 * 1024)
                {
                    return Err(private_network_error());
                }
                use wasmdroid_engine::driver::RuntimeDriver;
                let spec = serde_json::from_str(&spec_json)?;
                let session =
                    wasmdroid_engine::session::EngineSession::join_worker(data_dir, owner)?;
                let config = network_config
                    .map(|json| serde_json::from_str(&json).map_err(|_| private_network_error()))
                    .transpose()?;
                let inner = wasmdroid_wasmer_local::service_driver::ServiceDriver::with_network(
                    session,
                    instance_id.clone(),
                    generation,
                    std::sync::Arc::new(PlatformControl(worker_control)),
                    config,
                );
                inner.create(&instance_id, &spec)?;
                inner.start(&instance_id)?;
                Ok(std::sync::Arc::new(Self {
                    inner: std::sync::Arc::new(inner),
                }))
            }
            #[cfg(not(feature = "runtime"))]
            {
                let _ = (
                    owner,
                    data_dir,
                    instance_id,
                    generation,
                    spec_json,
                    worker_control,
                    network_config,
                );
                Err(BridgeError::Operation {
                    reason: "runtime is not included".into(),
                })
            }
        }
        #[cfg(not(unix))]
        {
            let _ = (
                data_dir,
                owner_descriptor,
                instance_id,
                generation,
                spec_json,
                worker_control,
                network_config,
            );
            Err(BridgeError::Operation {
                reason: "worker runtime requires Android/Unix".into(),
            })
        }
    }
    pub fn capabilities(&self) -> Result<String, BridgeError> {
        #[cfg(feature = "runtime")]
        {
            use wasmdroid_engine::driver::RuntimeDriver;
            Ok(serde_json::to_string(&self.inner.capabilities())?)
        }
        #[cfg(not(feature = "runtime"))]
        {
            Err(storage_lock_error())
        }
    }
    pub fn update_network(&self, json: String) -> Result<(), BridgeError> {
        if json.len() > 32 * 1024 {
            return Err(private_network_error());
        }
        #[cfg(feature = "runtime")]
        {
            let config = serde_json::from_str(&json).map_err(|_| private_network_error())?;
            Ok(self.inner.update_network(config)?)
        }
        #[cfg(not(feature = "runtime"))]
        {
            let _ = json;
            Err(storage_lock_error())
        }
    }
    pub fn enable_lan(
        &self,
        scope_json: String,
        binder: Box<dyn LanSocketBinder>,
    ) -> Result<String, BridgeError> {
        let scope = lan_control_scope(&scope_json)?;
        #[cfg(all(feature = "runtime", unix))]
        {
            use std::os::fd::AsRawFd;
            let limits = wasmdroid_engine::network::lan::LanLimits::new(
                16,
                std::time::Duration::from_secs(10),
            )?;
            let address =
                self.inner
                    .enable_lan(scope.clone(), 0, limits, move |socket, selected| {
                        if binder.bind_socket(
                            socket.as_raw_fd(),
                            selected.interface_epoch(),
                            selected.private_address().to_string(),
                        ) {
                            Ok(())
                        } else {
                            Err(wasmdroid_engine::EngineError::Unsupported(
                                "Selected Wi-Fi socket binding was refused".into(),
                            ))
                        }
                    })?;
            Ok(serde_json::json!({"instance_id":scope.instance_id(),"generation":scope.generation(),
                "instance_port":scope.guest_port(),"grant_epoch":scope.interface_epoch(),
                "private_address":address.ip().to_string(),
                "ipv6_address":scope.ipv6().map(|value| value.address().to_string()),
                "host_port":address.port(),"phase":"active"}).to_string())
        }
        #[cfg(not(all(feature = "runtime", unix)))]
        {
            let _ = (scope, binder);
            Err(BridgeError::Operation {
                reason: "LAN runtime is not included".into(),
            })
        }
    }
    /// Fast admission fence; actual cleanup is acknowledged only by disable_lan
    /// or the owner's observed worker death. This method never fabricates drain.
    pub fn revoke_lan(&self, scope_json: String) -> Result<(), BridgeError> {
        let scope = lan_control_scope(&scope_json)?;
        #[cfg(feature = "runtime")]
        {
            Ok(self.inner.revoke_lan(&scope)?)
        }
        #[cfg(not(feature = "runtime"))]
        {
            let _ = scope;
            Err(storage_lock_error())
        }
    }
    pub fn revoke_all_lan(&self) -> Result<(), BridgeError> {
        #[cfg(feature = "runtime")]
        {
            Ok(self.inner.revoke_all_lan()?)
        }
        #[cfg(not(feature = "runtime"))]
        {
            Err(storage_lock_error())
        }
    }
    pub fn disable_lan(&self, scope_json: String) -> Result<String, BridgeError> {
        let scope = lan_control_scope(&scope_json)?;
        #[cfg(feature = "runtime")]
        {
            let report = self.inner.disable_lan(&scope)?;
            let failed = self.inner.lan_status(&scope)?.failed;
            Ok(
                serde_json::json!({"phase":"closed","grant_epoch":scope.interface_epoch(),
                "accepted":report.accepted,"rejected":report.rejected,"completed":report.completed,
                "failed_connections":report.failed,"failed":failed,"cancelled":report.cancelled})
                .to_string(),
            )
        }
        #[cfg(not(feature = "runtime"))]
        {
            let _ = scope;
            Err(storage_lock_error())
        }
    }
    pub fn lan_status(&self, scope_json: String) -> Result<String, BridgeError> {
        let scope = lan_control_scope(&scope_json)?;
        #[cfg(feature = "runtime")]
        {
            use wasmdroid_wasmer_local::service_lan::LanPhase;
            let status = self.inner.lan_status(&scope)?;
            let phase = match status.phase {
                LanPhase::Preparing => "preparing",
                LanPhase::Active => "active",
                LanPhase::Revoking => "revoking",
                LanPhase::Closed => "closed",
                LanPhase::Uncertain => "uncertain",
            };
            let active = status.address.filter(|_| phase == "active");
            Ok(serde_json::json!({"phase":phase,"grant_epoch":scope.interface_epoch(),"failed":status.failed,
                "private_address":active.map(|address|address.ip().to_string()),
                "ipv6_address":active.and_then(|_|scope.ipv6()).map(|value|value.address().to_string()),
                "host_port":active.map(|address|address.port())}).to_string())
        }
        #[cfg(not(feature = "runtime"))]
        {
            let _ = scope;
            Err(storage_lock_error())
        }
    }
    pub fn info(&self) -> Result<String, BridgeError> {
        #[cfg(feature = "runtime")]
        {
            Ok(serde_json::to_string(&self.inner.info()?)?)
        }
        #[cfg(not(feature = "runtime"))]
        {
            Err(storage_lock_error())
        }
    }
    pub fn stats(&self) -> Result<String, BridgeError> {
        #[cfg(feature = "runtime")]
        {
            use wasmdroid_engine::driver::RuntimeDriver;
            Ok(serde_json::to_string(&self.inner.stats(self.inner.id())?)?)
        }
        #[cfg(not(feature = "runtime"))]
        {
            Err(storage_lock_error())
        }
    }
    pub fn poll(&self) -> Result<Option<String>, BridgeError> {
        #[cfg(feature = "runtime")]
        {
            self.inner
                .poll()?
                .map(|result| serde_json::to_string(&result).map_err(BridgeError::from))
                .transpose()
        }
        #[cfg(not(feature = "runtime"))]
        {
            Err(storage_lock_error())
        }
    }
    pub fn request_stop(&self, grace_seconds: u64) -> Result<(), BridgeError> {
        #[cfg(feature = "runtime")]
        {
            use wasmdroid_engine::driver::{RuntimeDriver, StopSignal};
            Ok(self
                .inner
                .stop(self.inner.id(), StopSignal::Terminate, grace_seconds)?)
        }
        #[cfg(not(feature = "runtime"))]
        {
            let _ = grace_seconds;
            Err(storage_lock_error())
        }
    }
    pub fn kill(&self) -> Result<(), BridgeError> {
        #[cfg(feature = "runtime")]
        {
            use wasmdroid_engine::driver::RuntimeDriver;
            Ok(self.inner.kill(self.inner.id())?)
        }
        #[cfg(not(feature = "runtime"))]
        {
            Err(storage_lock_error())
        }
    }
    pub fn write_stdin(&self, bytes: Vec<u8>) -> Result<(), BridgeError> {
        #[cfg(feature = "runtime")]
        {
            use wasmdroid_engine::driver::RuntimeDriver;
            Ok(self.inner.attach_stdio(self.inner.id(), &bytes)?)
        }
        #[cfg(not(feature = "runtime"))]
        {
            let _ = bytes;
            Err(storage_lock_error())
        }
    }
    pub fn terminal_frame(&self, query: String) -> Result<String, BridgeError> {
        #[cfg(feature = "runtime")]
        {
            Ok(self.inner.terminal_frame(&query)?)
        }
        #[cfg(not(feature = "runtime"))]
        {
            let _ = query;
            Err(storage_lock_error())
        }
    }
    pub fn close_stdin(&self) -> Result<(), BridgeError> {
        #[cfg(feature = "runtime")]
        {
            Ok(self.inner.close_stdin()?)
        }
        #[cfg(not(feature = "runtime"))]
        {
            Err(storage_lock_error())
        }
    }
    pub fn interrupt(&self) -> Result<(), BridgeError> {
        #[cfg(feature = "runtime")]
        {
            Ok(self.inner.interrupt()?)
        }
        #[cfg(not(feature = "runtime"))]
        {
            Err(storage_lock_error())
        }
    }
    pub fn resize_terminal(
        &self,
        cols: u32,
        rows: u32,
        width: u32,
        height: u32,
    ) -> Result<(), BridgeError> {
        #[cfg(feature = "runtime")]
        {
            Ok(self.inner.resize(cols, rows, width, height)?)
        }
        #[cfg(not(feature = "runtime"))]
        {
            let _ = (cols, rows, width, height);
            Err(storage_lock_error())
        }
    }
}

#[uniffi::export]
pub fn run_storage_probe(data_dir: String) -> Result<String, BridgeError> {
    Ok(serde_json::to_string(
        &wasmdroid_engine::storage_probe::run(std::path::Path::new(&data_dir))?,
    )?)
}

/// Finite cancellation acceptance probe, not arbitrary guest execution.
#[uniffi::export]
pub fn run_cancellation_probe() -> Result<String, BridgeError> {
    #[cfg(feature = "runtime")]
    {
        wasmdroid_wasmer_local::cancellation::run_probe()
            .and_then(|result| Ok(serde_json::to_string(&result)?))
            .map_err(|error| BridgeError::Operation {
                reason: error.to_string(),
            })
    }
    #[cfg(not(feature = "runtime"))]
    {
        Err(BridgeError::Operation {
            reason: "Wasmer runtime is not included".into(),
        })
    }
}

#[derive(Debug, uniffi::Record)]
pub struct EngineInfo {
    pub api_version: u32,
    pub version: String,
    pub endpoint_id: String,
    pub runtime_probe_available: bool,
    pub runtime_backend: String,
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum BridgeError {
    #[error("{reason}")]
    Operation { reason: String },
}

#[uniffi::export]
pub fn engine_info() -> EngineInfo {
    EngineInfo {
        api_version: wasmdroid_engine::api::API_VERSION,
        version: env!("CARGO_PKG_VERSION").into(),
        endpoint_id: "local".into(),
        runtime_probe_available: cfg!(feature = "runtime"),
        runtime_backend: {
            #[cfg(feature = "runtime")]
            {
                wasmdroid_wasmer_local::benchmark::compiler_name().into()
            }
            #[cfg(not(feature = "runtime"))]
            {
                "unavailable".into()
            }
        },
    }
}

/// Explicit fixed-workload benchmark. Invoke off the main thread. The returned
/// JSON records raw samples and units; no performance threshold is a test gate.
#[uniffi::export]
pub fn run_registry_benchmark(
    data_dir: String,
    observer: Option<Box<dyn ProbeObserver>>,
) -> Result<String, BridgeError> {
    #[cfg(feature = "runtime")]
    {
        let result = (|| -> Result<String, Box<dyn std::error::Error>> {
            use wasmdroid_engine::{
                api::ArtifactReference,
                installer::{InstallLimits, Installer},
                registry::wasmer::WasmerRegistry,
                state::StateDb,
                store::ContentStore,
            };
            let root = std::path::Path::new(&data_dir);
            std::fs::create_dir_all(root)?;
            let registry = WasmerRegistry::new()?;
            let store = ContentStore::open(root.join("store"))?;
            let state = StateDb::open(root.join("engine.sqlite"))?;
            let installer = Installer {
                registry: &registry,
                store: &store,
                state: &state,
                limits: InstallLimits::default(),
            };
            if let Some(observer) = &observer {
                observer.on_phase("benchmark_registry_setup_excluded_from_timings".into());
            }
            let graph = installer.pull(
                &ArtifactReference::parse("wasmer.io/python/python@3.13.20")?,
                &mut |_| {},
            )?;
            let result = wasmdroid_wasmer_local::benchmark::python(
                &store,
                &graph.root,
                &graph.images,
                &mut |phase| {
                    if let Some(observer) = &observer {
                        observer.on_phase(phase.into());
                    }
                },
            )?;
            Ok(serde_json::to_string(&result)?)
        })();
        result.map_err(|error| BridgeError::Operation {
            reason: error.to_string(),
        })
    }
    #[cfg(not(feature = "runtime"))]
    {
        let _ = (data_dir, observer);
        Err(BridgeError::Operation {
            reason: "Wasmer runtime is not included".into(),
        })
    }
}

#[uniffi::export]
pub fn run_core_benchmark() -> Result<String, BridgeError> {
    #[cfg(feature = "runtime")]
    {
        wasmdroid_wasmer_local::benchmark::core()
            .and_then(|result| Ok(serde_json::to_string(&result)?))
            .map_err(|error| BridgeError::Operation {
                reason: error.to_string(),
            })
    }
    #[cfg(not(feature = "runtime"))]
    {
        Err(BridgeError::Operation {
            reason: "Wasmer runtime is not included".into(),
        })
    }
}

/// Boundary uses the versioned, serializable catalog contract. No UI policy here.
#[uniffi::export]
pub fn validate_catalog(json: String) -> Result<u32, BridgeError> {
    wasmdroid_engine::catalog::Catalog::parse(&json)
        .map(|catalog| catalog.packages.len() as u32)
        .map_err(|err| BridgeError::Operation {
            reason: err.to_string(),
        })
}

/// Finite built-in WASI probe only. Dispatch off the Android main thread.
#[uniffi::export]
pub fn run_wasi_probe() -> Result<String, BridgeError> {
    #[cfg(feature = "runtime")]
    {
        wasmdroid_wasmer_local::run_probe().map_err(|err| BridgeError::Operation {
            reason: err.to_string(),
        })
    }
    #[cfg(not(feature = "runtime"))]
    {
        Err(BridgeError::Operation {
            reason: "This build does not include the Wasmer runtime probe".into(),
        })
    }
}

#[derive(Debug, uniffi::Record)]
pub struct RegistryProbeResult {
    pub reference: String,
    pub digest: String,
    pub installed_images: u32,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub elapsed_ms: u64,
}

#[uniffi::export(callback_interface)]
pub trait ProbeObserver: Send + Sync {
    fn on_phase(&self, phase: String);
}

/// Explicit live Stage 0 acceptance harness, not a user-facing arbitrary runner.
/// Pulls the pinned Python release and transitive dependencies, then executes
/// only the fixed sandbox regression script without host networking or mounts.
#[uniffi::export]
pub fn run_registry_python_probe(
    data_dir: String,
    observer: Option<Box<dyn ProbeObserver>>,
) -> Result<RegistryProbeResult, BridgeError> {
    #[cfg(feature = "runtime")]
    {
        registry_python_probe(&data_dir, observer.as_deref()).map_err(|error| {
            BridgeError::Operation {
                reason: error.to_string(),
            }
        })
    }
    #[cfg(not(feature = "runtime"))]
    {
        let _ = (data_dir, observer);
        Err(BridgeError::Operation {
            reason: "Wasmer runtime is not included".into(),
        })
    }
}

#[cfg(feature = "runtime")]
fn registry_python_probe(
    data_dir: &str,
    observer: Option<&dyn ProbeObserver>,
) -> Result<RegistryProbeResult, Box<dyn std::error::Error>> {
    use wasmdroid_engine::{
        api::ArtifactReference,
        installer::{InstallLimits, Installer},
        registry::wasmer::WasmerRegistry,
        state::StateDb,
        store::ContentStore,
    };
    let root = std::path::Path::new(data_dir);
    std::fs::create_dir_all(root)?;
    let registry = WasmerRegistry::new()?;
    let store = ContentStore::open(root.join("store"))?;
    let state = StateDb::open(root.join("engine.sqlite"))?;
    let installer = Installer {
        registry: &registry,
        store: &store,
        state: &state,
        limits: InstallLimits::default(),
    };
    if let Some(observer) = observer {
        observer.on_phase("resolving_registry".into());
    }
    let graph = installer.pull(
        &ArtifactReference::parse("wasmer.io/python/python@3.13.20")?,
        &mut |progress| {
            if progress.bytes_downloaded == 0
                && let Some(observer) = observer
            {
                observer.on_phase(format!(
                    "{:?}:{}",
                    progress.phase,
                    progress.reference.as_str()
                ));
            }
        },
    )?;
    let script = include_str!("../../../fixtures/python-sandbox-probe.py");
    let result = wasmdroid_wasmer_local::webc_execution::run_offline_with_progress(
        &store,
        &graph.root,
        &graph.images,
        "python",
        &["-c".into(), script.into()],
        &mut |phase| {
            if let Some(observer) = observer {
                observer.on_phase(phase.into());
            }
        },
    )?;
    Ok(RegistryProbeResult {
        reference: graph.root.reference.as_str().into(),
        digest: graph.root.digest.as_str().into(),
        installed_images: graph.images.len() as u32,
        stdout: result.stdout,
        stderr: result.stderr,
        exit_code: result.exit_code,
        elapsed_ms: result.elapsed_ms,
    })
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct HttpProbeInfo {
    pub reference: String,
    pub digest: String,
    pub installed_images: u32,
    pub url: String,
    pub document_token: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct HttpProbeStopResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub elapsed_ms: u64,
    pub forced: bool,
    pub active_execution_tasks: u32,
}

/// Explicit live HTTP acceptance instance, owned until stop/drop. Product
/// lifecycle and Android service ownership will use the neutral driver API.
#[derive(uniffi::Object)]
pub struct RegistryHttpProbe {
    info: HttpProbeInfo,
    #[cfg(feature = "runtime")]
    inner: std::sync::Mutex<wasmdroid_wasmer_local::http_execution::HttpInstance>,
}

#[uniffi::export]
impl RegistryHttpProbe {
    pub fn info(&self) -> HttpProbeInfo {
        self.info.clone()
    }

    pub fn stop(&self) -> Result<HttpProbeStopResult, BridgeError> {
        #[cfg(feature = "runtime")]
        {
            let mut inner = self.inner.lock().map_err(|_| BridgeError::Operation {
                reason: "HTTP instance lock failed".into(),
            })?;
            let result = inner.stop().map_err(|error| BridgeError::Operation {
                reason: error.to_string(),
            })?;
            Ok(HttpProbeStopResult {
                stdout: result.execution.stdout,
                stderr: result.execution.stderr,
                exit_code: result.execution.exit_code,
                elapsed_ms: result.execution.elapsed_ms,
                forced: result.forced,
                active_execution_tasks: result.active_execution_tasks,
            })
        }
        #[cfg(not(feature = "runtime"))]
        {
            Err(BridgeError::Operation {
                reason: "Wasmer runtime is not included".into(),
            })
        }
    }
}

#[uniffi::export]
pub fn start_registry_http_probe(
    data_dir: String,
    observer: Option<Box<dyn ProbeObserver>>,
) -> Result<std::sync::Arc<RegistryHttpProbe>, BridgeError> {
    #[cfg(feature = "runtime")]
    {
        registry_http_probe(&data_dir, observer.as_deref())
            .map(std::sync::Arc::new)
            .map_err(|error| BridgeError::Operation {
                reason: error.to_string(),
            })
    }
    #[cfg(not(feature = "runtime"))]
    {
        let _ = (data_dir, observer);
        Err(BridgeError::Operation {
            reason: "Wasmer runtime is not included".into(),
        })
    }
}

#[cfg(feature = "runtime")]
fn registry_http_probe(
    data_dir: &str,
    observer: Option<&dyn ProbeObserver>,
) -> Result<RegistryHttpProbe, Box<dyn std::error::Error>> {
    use wasmdroid_engine::{
        api::ArtifactReference,
        installer::{InstallLimits, Installer},
        registry::wasmer::WasmerRegistry,
        state::StateDb,
        store::ContentStore,
    };
    let root = std::path::Path::new(data_dir);
    std::fs::create_dir_all(root)?;
    let registry = WasmerRegistry::new()?;
    let store = ContentStore::open(root.join("store"))?;
    let state = StateDb::open(root.join("engine.sqlite"))?;
    let installer = Installer {
        registry: &registry,
        store: &store,
        state: &state,
        limits: InstallLimits::default(),
    };
    let graph = installer.pull(
        &ArtifactReference::parse("wasmer.io/wasmer/static-web-server@2.44.0")?,
        &mut |progress| {
            if progress.bytes_downloaded == 0
                && let Some(observer) = observer
            {
                observer.on_phase(format!(
                    "{:?}:{}",
                    progress.phase,
                    progress.reference.as_str()
                ));
            }
        },
    )?;
    let inner = wasmdroid_wasmer_local::http_execution::HttpInstance::start(
        &store,
        &graph.root,
        &graph.images,
        &mut |phase| {
            if let Some(observer) = observer {
                observer.on_phase(phase.into());
            }
        },
    )?;
    let info = HttpProbeInfo {
        reference: graph.root.reference.as_str().into(),
        digest: graph.root.digest.as_str().into(),
        installed_images: graph.images.len() as u32,
        url: inner.url().into(),
        document_token: inner.document_token().into(),
    };
    Ok(RegistryHttpProbe {
        info,
        inner: std::sync::Mutex::new(inner),
    })
}
