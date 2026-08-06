//! Headless navigation host demonstrating the UnitCompose V0 lifecycle.
//!
//! [`build_from_path`] and [`build_from_source`] parse and prepare one of the
//! example Module Definitions. The returned [`PreparedNavigation`] exposes the
//! compiled graph and fixed description, supports explicit [`PreparedNavigation::warm_up`],
//! and executes through the checked profiled route. [`NavigationHost::reload`]
//! prepares and warms a candidate before atomically replacing the active
//! Module, leaving the old Module available to its owner.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use unit_compose_core::{
    AllocationCapability, AllocationDomain, AllocationEvidence, BoundedBufferWriter,
    BoundedStorage, BuildOptions, CapacityError, CompiledGraph, FixedModuleDescription, Module,
    ModuleInput, PortDescriptor, PreparedInputPlan, PreparedInputSpec, RequirementStatus,
    ResourceDescriptor, ResourceId, ResourceRegistry, RunError, SemanticType, Unit,
    UnitConfigurationSummary, UnitDescriptor, UnitId, UnitRegistry, UnitTypeName, UnitWorkspace,
    plan_storage,
};
use unit_compose_yaml::{
    BoundSources, CompiledDefinition, FrontendRegistry, ParseLimits, UnitRequirements, load,
};

pub const MAX_CELLS: usize = 256;
pub const MAX_PATH: usize = 64;
const PLAN_TOKEN: u64 = 0x4e41_5635;
const INF: u32 = u32::MAX / 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GridPoint {
    pub x: u16,
    pub y: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RosOccupancyGrid {
    pub width: usize,
    pub height: usize,
    pub data: Vec<i8>,
    pub start: GridPoint,
    pub goal: GridPoint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SearchAlgorithm {
    AStar,
    Dijkstra,
}

#[derive(Clone, Debug, Deserialize)]
struct DecoderConfig {
    max_cells: usize,
}

#[derive(Clone, Debug, Deserialize)]
struct InflationConfig {
    radius: usize,
    max_cells: usize,
}

#[derive(Clone, Debug, Deserialize)]
struct PlannerConfig {
    max_cells: usize,
    max_path: usize,
    max_expansions: usize,
}

#[derive(Clone, Debug, Deserialize)]
struct SmootherConfig {
    max_path: usize,
}

#[derive(Clone, Debug, Deserialize)]
struct EmptyConfig {}

pub struct PreparedNavigation {
    pub graph: CompiledGraph,
    pub description: FixedModuleDescription,
    pub input_plan: PreparedInputPlan,
    pub module: Module<NavigationUnit>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExecutionEvidence {
    pub decoder: usize,
    pub inflation: usize,
    pub planner: usize,
    pub stats: usize,
    pub smoother: usize,
    pub occupied_cost_map_cells: Option<usize>,
}

impl PreparedNavigation {
    pub fn warm_up(&mut self, input: &RosOccupancyGrid) -> Result<(), RunError> {
        self.module.warm_up(input).map(|_| ())
    }

    pub fn supplied_input<T: 'static>(&self, capacity: usize) -> ModuleInput {
        ModuleInput::of::<T>(
            ResourceId::new("occupancy_grid"),
            grid_type(),
            capacity,
            PLAN_TOKEN,
        )
    }

    pub fn run_checked_profiled(
        &mut self,
        supplied: &[ModuleInput],
        input: &RosOccupancyGrid,
        probes: &mut [&mut dyn unit_compose_core::AllocationDomainProbe],
    ) -> Result<Vec<GridPoint>, RunError> {
        self.input_plan
            .validate(supplied)
            .map_err(RunError::Input)?;
        let view = self.module.run_profiled(input, probes, None)?;
        Ok(view.to_vec())
    }

    pub fn execution_evidence(&self) -> ExecutionEvidence {
        self.module.unit().execution_evidence()
    }
}

pub struct NavigationHost {
    active: PreparedNavigation,
}

impl NavigationHost {
    pub const fn new(active: PreparedNavigation) -> Self {
        Self { active }
    }

    pub const fn active(&self) -> &PreparedNavigation {
        &self.active
    }

    pub fn active_mut(&mut self) -> &mut PreparedNavigation {
        &mut self.active
    }

    pub fn activate(&mut self, candidate: PreparedNavigation) -> PreparedNavigation {
        std::mem::replace(&mut self.active, candidate)
    }

    pub fn reload(
        &mut self,
        path: &Path,
        warm_input: &RosOccupancyGrid,
    ) -> Result<PreparedNavigation, String> {
        let mut candidate = build_from_path(path)?;
        candidate
            .warm_up(warm_input)
            .map_err(|error| format!("candidate warm-up failed: {error:?}"))?;
        Ok(self.activate(candidate))
    }
}

pub struct NavigationUnit {
    algorithm: SearchAlgorithm,
    smoothing: bool,
    inflation_radius: usize,
    max_cells: usize,
    max_path: usize,
    max_expansions: usize,
    cost_map: Vec<u8>,
    distance: Vec<u32>,
    parent: Vec<usize>,
    closed: Vec<bool>,
    raw_path: Vec<GridPoint>,
    smooth_path: Vec<GridPoint>,
    evidence: ExecutionEvidence,
}

impl NavigationUnit {
    fn from_definition(definition: &CompiledDefinition) -> Result<Self, String> {
        let planner = definition
            .graph
            .units
            .iter()
            .find(|unit| unit.id.as_str() == "plan")
            .ok_or_else(|| "missing plan Unit".to_owned())?;
        let algorithm = match planner.unit_type.as_str() {
            "nav.astar/v1" => SearchAlgorithm::AStar,
            "nav.dijkstra/v1" => SearchAlgorithm::Dijkstra,
            other => return Err(format!("unsupported planner {other}")),
        };
        let planner_config = definition
            .config::<PlannerConfig>(&UnitId::new("plan"))
            .ok_or_else(|| "missing planner configuration".to_owned())?;
        let inflation = definition
            .config::<InflationConfig>(&UnitId::new("inflate"))
            .ok_or_else(|| "missing inflation configuration".to_owned())?;
        let decoder = definition
            .config::<DecoderConfig>(&UnitId::new("decode"))
            .ok_or_else(|| "missing decoder configuration".to_owned())?;
        let smoothing = definition
            .graph
            .units
            .iter()
            .any(|unit| unit.unit_type.as_str() == "nav.line_of_sight_smoother/v1");
        if decoder.max_cells != planner_config.max_cells
            || inflation.max_cells != planner_config.max_cells
        {
            return Err("all map/search bounds must agree".to_owned());
        }
        if planner_config.max_cells == 0
            || planner_config.max_path == 0
            || planner_config.max_expansions == 0
        {
            return Err("navigation bounds must be non-zero".to_owned());
        }
        if inflation.radius > planner_config.max_cells {
            return Err("inflation radius exceeds the prepared map bound".to_owned());
        }
        if smoothing {
            let smoother = definition
                .config::<SmootherConfig>(&UnitId::new("smooth"))
                .ok_or_else(|| "missing smoother configuration".to_owned())?;
            if smoother.max_path != planner_config.max_path {
                return Err("planner and smoother path bounds must agree".to_owned());
            }
        }
        Ok(Self {
            algorithm,
            smoothing,
            inflation_radius: inflation.radius,
            max_cells: planner_config.max_cells,
            max_path: planner_config.max_path,
            max_expansions: planner_config.max_expansions,
            cost_map: vec![0; planner_config.max_cells],
            distance: vec![INF; planner_config.max_cells],
            parent: vec![usize::MAX; planner_config.max_cells],
            closed: vec![false; planner_config.max_cells],
            raw_path: Vec::with_capacity(planner_config.max_path),
            smooth_path: Vec::with_capacity(planner_config.max_path),
            evidence: ExecutionEvidence::default(),
        })
    }

    fn validate_grid(&self, input: &RosOccupancyGrid) -> Result<usize, RunError> {
        let cells = input
            .width
            .checked_mul(input.height)
            .ok_or(RunError::InvalidInput {
                message: "grid dimensions overflow",
            })?;
        if cells == 0 || cells > self.max_cells || input.data.len() != cells {
            return Err(RunError::InvalidInput {
                message: "occupancy grid exceeds prepared bounds or has invalid length",
            });
        }
        if usize::from(input.start.x) >= input.width
            || usize::from(input.start.y) >= input.height
            || usize::from(input.goal.x) >= input.width
            || usize::from(input.goal.y) >= input.height
        {
            return Err(RunError::InvalidInput {
                message: "start or goal is outside the occupancy grid",
            });
        }
        Ok(cells)
    }

    fn decode(&mut self, input: &RosOccupancyGrid, cells: usize) {
        self.evidence.decoder += 1;
        for (target, occupancy) in self.cost_map[..cells].iter_mut().zip(&input.data) {
            *target = u8::from(*occupancy < 0 || *occupancy >= 50);
        }
    }

    fn inflate(&mut self, input: &RosOccupancyGrid, cells: usize) {
        self.evidence.inflation += 1;
        if self.inflation_radius == 0 {
            return;
        }
        for index in 0..cells {
            if input.data[index] < 0 || input.data[index] >= 50 {
                let x = index % input.width;
                let y = index / input.width;
                let min_x = x.saturating_sub(self.inflation_radius);
                let max_x = (x + self.inflation_radius).min(input.width - 1);
                let min_y = y.saturating_sub(self.inflation_radius);
                let max_y = (y + self.inflation_radius).min(input.height - 1);
                for iy in min_y..=max_y {
                    for ix in min_x..=max_x {
                        self.cost_map[iy * input.width + ix] = 1;
                    }
                }
            }
        }
    }

    fn search(&mut self, input: &RosOccupancyGrid, cells: usize) -> Result<(), RunError> {
        self.evidence.planner += 1;
        self.distance[..cells].fill(INF);
        self.parent[..cells].fill(usize::MAX);
        self.closed[..cells].fill(false);
        self.raw_path.clear();
        let start = usize::from(input.start.y) * input.width + usize::from(input.start.x);
        let goal = usize::from(input.goal.y) * input.width + usize::from(input.goal.x);
        if self.cost_map[start] != 0 || self.cost_map[goal] != 0 {
            return Err(RunError::InvalidInput {
                message: "start or goal is occupied after inflation",
            });
        }
        self.distance[start] = 0;
        let mut expansions = 0;
        loop {
            let mut best = usize::MAX;
            let mut best_score = INF;
            for index in 0..cells {
                if self.closed[index] || self.distance[index] == INF {
                    continue;
                }
                let score = self.distance[index].saturating_add(match self.algorithm {
                    SearchAlgorithm::Dijkstra => 0,
                    SearchAlgorithm::AStar => manhattan(index, goal, input.width),
                });
                if score < best_score || (score == best_score && index < best) {
                    best = index;
                    best_score = score;
                }
            }
            if best == usize::MAX || best == goal {
                break;
            }
            if expansions == self.max_expansions {
                return Err(capacity(
                    "search_workspace",
                    expansions + 1,
                    self.max_expansions,
                ));
            }
            expansions += 1;
            self.closed[best] = true;
            let x = best % input.width;
            let y = best / input.width;
            for neighbor in neighbors(x, y, input.width, input.height)
                .into_iter()
                .flatten()
            {
                if self.cost_map[neighbor] != 0 || self.closed[neighbor] {
                    continue;
                }
                let candidate = self.distance[best] + 1;
                if candidate < self.distance[neighbor] {
                    self.distance[neighbor] = candidate;
                    self.parent[neighbor] = best;
                }
            }
        }
        if self.distance[goal] == INF {
            self.raw_path.clear();
            return Ok(());
        }
        let mut current = goal;
        loop {
            if self.raw_path.len() == self.max_path {
                return Err(capacity("raw_path", self.raw_path.len() + 1, self.max_path));
            }
            self.raw_path.push(GridPoint {
                x: (current % input.width) as u16,
                y: (current / input.width) as u16,
            });
            if current == start {
                break;
            }
            current = self.parent[current];
        }
        self.raw_path.reverse();
        Ok(())
    }

    fn compute_cost_map_stats(&mut self, cells: usize) {
        self.evidence.stats += 1;
        self.evidence.occupied_cost_map_cells = Some(
            self.cost_map[..cells]
                .iter()
                .filter(|cell| **cell != 0)
                .count(),
        );
    }

    fn smooth(&mut self, width: usize, height: usize) -> Result<(), RunError> {
        self.evidence.smoother += 1;
        self.smooth_path.clear();
        if self.raw_path.is_empty() {
            return Ok(());
        }
        let mut anchor = 0;
        self.smooth_path.push(self.raw_path[0]);
        while anchor + 1 < self.raw_path.len() {
            let mut next = self.raw_path.len() - 1;
            while next > anchor + 1
                && !line_is_clear(
                    self.raw_path[anchor],
                    self.raw_path[next],
                    &self.cost_map,
                    width,
                    height,
                )
            {
                next -= 1;
            }
            if self.smooth_path.len() == self.max_path {
                return Err(capacity(
                    "smoothed_path",
                    self.smooth_path.len() + 1,
                    self.max_path,
                ));
            }
            self.smooth_path.push(self.raw_path[next]);
            anchor = next;
        }
        Ok(())
    }

    const fn execution_evidence(&self) -> ExecutionEvidence {
        self.evidence
    }
}

impl Unit for NavigationUnit {
    type Input = RosOccupancyGrid;
    type Storage = BoundedStorage<GridPoint>;

    fn workspace_requirement(&self) -> usize {
        self.max_cells * (size_of::<u32>() + size_of::<usize>() + 1)
    }

    fn output_storage(&self) -> Self::Storage {
        BoundedStorage::new("path", self.max_path)
    }

    fn allocation_capability(&self) -> AllocationCapability {
        AllocationCapability::inspect(
            vec![AllocationDomain {
                name: "rust-global".to_owned(),
                evidence: AllocationEvidence::Instrumented,
            }],
            true,
        )
    }

    fn requirement_status(&self) -> RequirementStatus {
        RequirementStatus::Bounded
    }

    fn validate_input(&self, input: &Self::Input) -> Result<(), RunError> {
        self.validate_grid(input).map(|_| ())
    }

    fn run(
        &mut self,
        input: &Self::Input,
        output: &mut BoundedBufferWriter<'_, GridPoint>,
        mut workspace: UnitWorkspace<'_>,
    ) -> Result<(), RunError> {
        let cells = self.validate_grid(input)?;
        if workspace.len() < cells * (size_of::<u32>() + size_of::<usize>() + 1) {
            return Err(RunError::InvalidInput {
                message: "search workspace is smaller than the prepared bound",
            });
        }
        workspace.bytes().fill(0);
        self.decode(input, cells);
        self.inflate(input, cells);
        self.search(input, cells)?;
        self.compute_cost_map_stats(cells);
        if self.smoothing {
            self.smooth(input.width, input.height)?;
        }
        let path = if self.smoothing {
            &self.smooth_path
        } else {
            &self.raw_path
        };
        for point in path {
            output.try_push(*point).map_err(RunError::Capacity)?;
        }
        output.complete();
        Ok(())
    }
}

pub fn build_from_path(path: &Path) -> Result<PreparedNavigation, String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    build_from_source(&source)
}

pub fn build_from_source(source: &str) -> Result<PreparedNavigation, String> {
    let (units, resources, frontend) = registries()?;
    let bounds = BoundSources {
        host: BTreeMap::from([(ResourceId::new("occupancy_grid"), MAX_CELLS)]),
        adapters: BTreeMap::new(),
    };
    let definition = load(
        source,
        ParseLimits::default(),
        &frontend,
        &units,
        &resources,
        &bounds,
    )
    .map_err(|error| error.to_string())?
    .compile()
    .map_err(|error| error.to_string())?;
    validate_graph(&definition.graph)?;
    let storage = plan_storage(&definition.graph, &resources, &definition.requirements)
        .map_err(|error| format!("storage planning failed: {error:?}"))?;
    let configurations = configuration_summaries(&definition)?;
    let unit = NavigationUnit::from_definition(&definition)?;
    let module = Module::build(unit, BuildOptions::strict())
        .map_err(|error| format!("strict Module build failed: {error:?}"))?;
    let input_plan = PreparedInputPlan::new([PreparedInputSpec::of::<RosOccupancyGrid>(
        ResourceId::new("occupancy_grid"),
        grid_type(),
        MAX_CELLS,
        PLAN_TOKEN,
    )])
    .map_err(|error| format!("input plan failed: {error:?}"))?;
    let description = FixedModuleDescription::new(
        definition.graph.clone(),
        configurations,
        definition.requirements,
        definition.workspace_bytes,
        storage.report().clone(),
        module.description().clone(),
        Vec::new(),
    );
    Ok(PreparedNavigation {
        graph: definition.graph,
        description,
        input_plan,
        module,
    })
}

fn configuration_summaries(
    definition: &CompiledDefinition,
) -> Result<Vec<UnitConfigurationSummary>, String> {
    definition
        .graph
        .units
        .iter()
        .map(|unit| {
            let summary = match unit.unit_type.as_str() {
                "nav.ros_map_decoder/v1" => {
                    let config = definition
                        .config::<DecoderConfig>(&unit.id)
                        .ok_or_else(|| format!("missing config for {}", unit.id.as_str()))?;
                    format!("max_cells={}", config.max_cells)
                }
                "nav.binary_inflation/v1" => {
                    let config = definition
                        .config::<InflationConfig>(&unit.id)
                        .ok_or_else(|| format!("missing config for {}", unit.id.as_str()))?;
                    format!("max_cells={},radius={}", config.max_cells, config.radius)
                }
                "nav.astar/v1" | "nav.dijkstra/v1" => {
                    let config = definition
                        .config::<PlannerConfig>(&unit.id)
                        .ok_or_else(|| format!("missing config for {}", unit.id.as_str()))?;
                    format!(
                        "max_cells={},max_expansions={},max_path={}",
                        config.max_cells, config.max_expansions, config.max_path
                    )
                }
                "nav.cost_map_stats/v1" => "{}".to_owned(),
                "nav.line_of_sight_smoother/v1" => {
                    let config = definition
                        .config::<SmootherConfig>(&unit.id)
                        .ok_or_else(|| format!("missing config for {}", unit.id.as_str()))?;
                    format!("max_path={}", config.max_path)
                }
                other => return Err(format!("no inspection summary for {other}")),
            };
            Ok(UnitConfigurationSummary {
                unit: unit.id.clone(),
                summary,
            })
        })
        .collect()
}

pub fn demo_grid() -> RosOccupancyGrid {
    let width = 12;
    let height = 10;
    let mut data = vec![0; width * height];
    for y in 1..9 {
        if !(4..=6).contains(&y) {
            data[y * width + 6] = 100;
        }
    }
    RosOccupancyGrid {
        width,
        height,
        data,
        start: GridPoint { x: 1, y: 1 },
        goal: GridPoint { x: 10, y: 8 },
    }
}

fn registries() -> Result<(UnitRegistry, ResourceRegistry, FrontendRegistry), String> {
    let grid = grid_type();
    let map = semantic("nav.BinaryMap/v1")?;
    let path = semantic("nav.Path/v1")?;
    let stats = semantic("nav.CostMapStats/v1")?;
    let mut resources = ResourceRegistry::default();
    resources
        .register(ResourceDescriptor::of::<RosOccupancyGrid>(
            grid.clone(),
            "bounded ROS occupancy-grid view",
            "dimensions and data length agree with host bound",
        ))
        .map_err(debug)?;
    resources
        .register(ResourceDescriptor::bounded_buffer::<Vec<u8>, u8>(
            map.clone(),
            "prepared binary-map buffer",
            "one byte per bounded cell",
        ))
        .map_err(debug)?;
    resources
        .register(ResourceDescriptor::bounded_buffer::<
            Vec<GridPoint>,
            GridPoint,
        >(
            path.clone(),
            "prepared path buffer",
            "bounded grid points",
        ))
        .map_err(debug)?;
    resources
        .register(ResourceDescriptor::of::<usize>(
            stats.clone(),
            "fixed cost statistics",
            "occupied-cell count",
        ))
        .map_err(debug)?;

    let mut units = UnitRegistry::default();
    register_unit(
        &mut units,
        "nav.ros_map_decoder/v1",
        vec![port::<RosOccupancyGrid>("grid", &grid)],
        vec![port::<Vec<u8>>("map", &map)],
    )?;
    register_unit(
        &mut units,
        "nav.binary_inflation/v1",
        vec![port::<Vec<u8>>("map", &map)],
        vec![port::<Vec<u8>>("cost_map", &map)],
    )?;
    for planner in ["nav.astar/v1", "nav.dijkstra/v1"] {
        register_unit(
            &mut units,
            planner,
            vec![port::<Vec<u8>>("cost_map", &map)],
            vec![port::<Vec<GridPoint>>("path", &path)],
        )?;
    }
    register_unit(
        &mut units,
        "nav.cost_map_stats/v1",
        vec![port::<Vec<u8>>("cost_map", &map)],
        vec![port::<usize>("stats", &stats)],
    )?;
    register_unit(
        &mut units,
        "nav.line_of_sight_smoother/v1",
        vec![
            port::<Vec<u8>>("cost_map", &map),
            port::<Vec<GridPoint>>("path", &path),
        ],
        vec![port::<Vec<GridPoint>>("path", &path)],
    )?;

    let mut frontend = FrontendRegistry::default();
    frontend
        .register::<DecoderConfig, _>(UnitTypeName::new("nav.ros_map_decoder/v1"), |config, _| {
            Ok(requirement("map", config.max_cells, config.max_cells))
        })
        .map_err(debug)?;
    frontend
        .register::<InflationConfig, _>(
            UnitTypeName::new("nav.binary_inflation/v1"),
            |config, _| Ok(requirement("cost_map", config.max_cells, config.max_cells)),
        )
        .map_err(debug)?;
    for planner in ["nav.astar/v1", "nav.dijkstra/v1"] {
        frontend
            .register::<PlannerConfig, _>(UnitTypeName::new(planner), |config, _| {
                Ok(requirement(
                    "path",
                    config.max_path,
                    config.max_cells * (size_of::<u32>() + size_of::<usize>() + 1),
                ))
            })
            .map_err(debug)?;
    }
    frontend
        .register::<EmptyConfig, _>(UnitTypeName::new("nav.cost_map_stats/v1"), |_, _| {
            Ok(requirement("stats", 1, 0))
        })
        .map_err(debug)?;
    frontend
        .register::<SmootherConfig, _>(
            UnitTypeName::new("nav.line_of_sight_smoother/v1"),
            |config, _| Ok(requirement("path", config.max_path, 0)),
        )
        .map_err(debug)?;
    Ok((units, resources, frontend))
}

fn validate_graph(graph: &CompiledGraph) -> Result<(), String> {
    let cost_map = graph
        .resources
        .iter()
        .find(|resource| resource.id.as_str() == "cost_map")
        .ok_or_else(|| "graph has no cost_map Resource".to_owned())?;
    if cost_map.consumers.len() < 2 {
        return Err("cost_map must fan out to planning and statistics".to_owned());
    }
    if graph.module_outputs.len() != 1 {
        return Err("navigation graph must publish exactly one path".to_owned());
    }
    Ok(())
}

fn register_unit(
    units: &mut UnitRegistry,
    name: &str,
    inputs: Vec<PortDescriptor>,
    outputs: Vec<PortDescriptor>,
) -> Result<(), String> {
    units
        .register(UnitDescriptor {
            type_name: UnitTypeName::new(name),
            inputs,
            outputs,
        })
        .map_err(debug)
}

fn requirement(output: &str, capacity: usize, workspace_bytes: usize) -> UnitRequirements {
    UnitRequirements {
        output_capacities: BTreeMap::from([(output.to_owned(), capacity)]),
        workspace_bytes,
    }
}

fn port<T: 'static>(name: &str, semantic_type: &SemanticType) -> PortDescriptor {
    PortDescriptor::of::<T>(name, semantic_type.clone())
}

fn semantic(name: &str) -> Result<SemanticType, String> {
    SemanticType::new(name).map_err(debug)
}

fn grid_type() -> SemanticType {
    SemanticType::new("nav.RosOccupancyGrid/v1").expect("static semantic type is valid")
}

fn debug(error: impl std::fmt::Debug) -> String {
    format!("{error:?}")
}

fn capacity(resource: &'static str, required: usize, prepared: usize) -> RunError {
    RunError::Capacity(CapacityError {
        resource,
        required,
        prepared,
        policy: unit_compose_core::CapacityPolicy::RejectOverflow,
    })
}

fn manhattan(index: usize, goal: usize, width: usize) -> u32 {
    let (x, y) = (index % width, index / width);
    let (goal_x, goal_y) = (goal % width, goal / width);
    (x.abs_diff(goal_x) + y.abs_diff(goal_y)) as u32
}

fn neighbors(x: usize, y: usize, width: usize, height: usize) -> [Option<usize>; 4] {
    [
        (x > 0).then_some(y * width + x.saturating_sub(1)),
        (x + 1 < width).then_some(y * width + x + 1),
        (y > 0).then_some(y.saturating_sub(1) * width + x),
        (y + 1 < height).then_some((y + 1) * width + x),
    ]
}

fn line_is_clear(from: GridPoint, to: GridPoint, map: &[u8], width: usize, height: usize) -> bool {
    let mut x = i32::from(from.x);
    let mut y = i32::from(from.y);
    let target_x = i32::from(to.x);
    let target_y = i32::from(to.y);
    let dx = (target_x - x).abs();
    let sx = if x < target_x { 1 } else { -1 };
    let dy = -(target_y - y).abs();
    let sy = if y < target_y { 1 } else { -1 };
    let mut error = dx + dy;
    loop {
        if x < 0
            || y < 0
            || x as usize >= width
            || y as usize >= height
            || map[y as usize * width + x as usize] != 0
        {
            return false;
        }
        if x == target_x && y == target_y {
            return true;
        }
        let twice = 2 * error;
        if twice >= dy {
            error += dy;
            x += sx;
        }
        if twice <= dx {
            error += dx;
            y += sy;
        }
    }
}
