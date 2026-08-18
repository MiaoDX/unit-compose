//! Headless navigation host demonstrating the UnitCompose V0 lifecycle.
//!
//! [`build_from_path`] and [`build_from_source`] parse and prepare one of the
//! example Module Definitions. The returned [`PreparedNavigation`] exposes the
//! compiled graph and fixed description, supports explicit [`PreparedNavigation::warm_up`],
//! and executes through the checked profiled route. [`NavigationHost::reload`]
//! prepares and warms a candidate before atomically replacing the active
//! Module, leaving the old Module available to its owner.

use std::cmp::Reverse;
use std::collections::VecDeque;
use std::collections::{BTreeMap, BinaryHeap};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use unit_compose_core::{
    AllocationCapability, AllocationDomain, AllocationEvidence, BuildOptions, CapacityError,
    CompiledGraph, FixedModuleDescription, InputHandle, Module, ModuleInput, ModuleInputs,
    OutputHandle, PortDescriptor, PreparedInputPlan, PreparedInputSpec, PreparedModuleDescription,
    RegistrationInvocation, RequirementStatus, ResourceDescriptor, ResourceId, ResourceRegistry,
    RunError, SemanticType, UnitConfigurationSummary, UnitDescriptor, UnitRegistry, UnitTypeName,
    plan_storage,
};
use unit_compose_yaml::{
    BoundSources, CompiledDefinition, ParseLimits, UnitRequirements, load,
    register_unit as register_yaml_unit,
};

pub const MAX_CELLS: usize = 1_920;
pub const MAX_PATH: usize = 256;
pub const EPISODE_LEGS: usize = 1_000;
const INF: u32 = u32::MAX / 4;

const DEMO_WIDTH: usize = 48;
const DEMO_HEIGHT: usize = 40;

// Fixed, downsampled occupancy fixture derived from the Apache-2.0 TurtleBot3
// Navigation2 map at commit fc817ce3073af1d6032397c64504134882af5e9a.
// Rows are image-top first.
const DEMO_MAP_ROWS: [&str; DEMO_HEIGHT] = [
    "????????????????????????????????????????????????",
    "????????????????????????????????????????????????",
    "????????????????????????????????????????????????",
    "????????????????################????????????????",
    "????????????????#..............##???????????????",
    "???????????????##...............##??????????????",
    "????????????####.................##?????????????",
    "???????????##....................####???????????",
    "??????????##........................##??????????",
    "??????????##.........................##?????????",
    "?????????##...........................#?????????",
    "????????##......##.....###....###.....##????????",
    "????????#......####....###....###......##???????",
    "???????##...............#......##.......#???????",
    "??????##................................##??????",
    "??????#..................................#??????",
    "?????##..................................##?????",
    "????##..........##.....##.....##........##??????",
    "????#..........###.....###....###......##???????",
    "????##..........##.....###....###......##???????",
    "?????##.................................##??????",
    "??????#.................................##??????",
    "??????##................................##??????",
    "???????##...............................##??????",
    "????????#......###.....##.....###......##???????",
    "????????##.....###.....###....###.....##????????",
    "?????????#.............##.....##......##????????",
    "?????????##..........................##?????????",
    "??????????##........................##??????????",
    "???????????#........................##??????????",
    "???????????#####.................####???????????",
    "??????????????##................##??????????????",
    "???????????????##..............##???????????????",
    "????????????????################????????????????",
    "???????????????????????????????#????????????????",
    "????????????????????????????????????????????????",
    "????????????????????????????????????????????????",
    "????????????????????????????????????????????????",
    "????????????????????????????????????????????????",
    "????????????????????????????????????????????????",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct GridPoint {
    pub x: u16,
    pub y: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RouteDistanceBucket {
    Short,
    Medium,
    Long,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NavigationLeg {
    pub start: GridPoint,
    pub goal: GridPoint,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NavigationItinerary {
    pub legs: Vec<NavigationLeg>,
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

#[derive(Clone)]
struct GridMap {
    width: usize,
    height: usize,
    start: GridPoint,
    goal: GridPoint,
    len: usize,
    cells: [u8; MAX_CELLS],
}

struct DecoderUnit {
    max_cells: usize,
}

struct InflationUnit {
    radius: usize,
    max_cells: usize,
}

struct PlannerUnit {
    algorithm: SearchAlgorithm,
    max_cells: usize,
    max_path: usize,
    max_expansions: usize,
    distance: Vec<u32>,
    parent: Vec<usize>,
    closed: Vec<bool>,
    open: BinaryHeap<Reverse<(u32, usize, u32)>>,
    raw_path: Vec<GridPoint>,
}

struct SmootherUnit {
    max_path: usize,
    path: Vec<GridPoint>,
}

pub struct PreparedNavigation {
    pub graph: CompiledGraph,
    pub description: FixedModuleDescription,
    pub input_plan: PreparedInputPlan,
    pub module: Module,
    input: InputHandle<RosOccupancyGrid>,
    binary_map: OutputHandle<GridMap>,
    cost_map: OutputHandle<GridMap>,
    raw_path: OutputHandle<Vec<GridPoint>>,
    path: OutputHandle<Vec<GridPoint>>,
    last_dimensions: Option<(usize, usize)>,
    binary_snapshot: Vec<u8>,
    cost_snapshot: Vec<u8>,
    raw_snapshot: Vec<GridPoint>,
    path_snapshot: Vec<GridPoint>,
    smoothing: bool,
}

impl PreparedNavigation {
    pub fn warm_up(&mut self, input: &RosOccupancyGrid) -> Result<(), RunError> {
        let mut inputs = ModuleInputs::with_capacity(1);
        inputs
            .bind(&self.input, input)
            .map_err(|error| RunError::RuntimeBinding {
                message: format!("{error:?}"),
            })?;
        self.module.warm_up(&inputs)?;
        self.refresh_snapshot()
    }

    pub fn supplied_input<T: 'static>(&self, capacity: usize) -> ModuleInput {
        ModuleInput::of::<T>(
            ResourceId::new("occupancy_grid"),
            grid_type(),
            capacity,
            self.input.plan_token(),
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
        self.execute(input, probes, None)?;
        Ok(self.path_snapshot.clone())
    }

    pub fn run_profiled(
        &mut self,
        input: &RosOccupancyGrid,
        probes: &mut [&mut dyn unit_compose_core::AllocationDomainProbe],
        sink: Option<&mut dyn unit_compose_core::DiagnosticSink>,
    ) -> Result<Vec<GridPoint>, RunError> {
        self.execute(input, probes, sink)?;
        Ok(self.path_snapshot.clone())
    }

    pub fn run_checked(
        &mut self,
        supplied: &[ModuleInput],
        input: &RosOccupancyGrid,
    ) -> Result<Vec<GridPoint>, RunError> {
        self.input_plan
            .validate(supplied)
            .map_err(RunError::Input)?;
        self.execute(input, &mut [], None)?;
        Ok(self.path_snapshot.clone())
    }

    pub const fn report(&self) -> &unit_compose_core::RunReport {
        self.module.report()
    }

    pub fn set_reporting_enabled(&mut self, enabled: bool) {
        self.module.set_reporting_enabled(enabled);
    }

    pub fn post_run_snapshot(&self) -> Result<NavigationPostRunSnapshot<'_>, String> {
        let (width, height) = self
            .last_dimensions
            .ok_or_else(|| "navigation has no successful run to inspect".to_owned())?;
        Ok(NavigationPostRunSnapshot {
            width,
            height,
            binary_map: &self.binary_snapshot,
            cost_map: &self.cost_snapshot,
            raw_path: &self.raw_snapshot,
            smoothed_path: self.smoothing.then_some(self.path_snapshot.as_slice()),
            final_path: &self.path_snapshot,
        })
    }

    fn execute(
        &mut self,
        input: &RosOccupancyGrid,
        probes: &mut [&mut dyn unit_compose_core::AllocationDomainProbe],
        sink: Option<&mut dyn unit_compose_core::DiagnosticSink>,
    ) -> Result<(), RunError> {
        let mut inputs = ModuleInputs::with_capacity(1);
        inputs
            .bind(&self.input, input)
            .map_err(|error| RunError::RuntimeBinding {
                message: format!("{error:?}"),
            })?;
        self.module.run_profiled(&inputs, probes, sink)?;
        self.refresh_snapshot()
    }

    fn refresh_snapshot(&mut self) -> Result<(), RunError> {
        let binary = self.module.output(&self.binary_map)?;
        let cost = self.module.output(&self.cost_map)?;
        let raw = self.module.output(&self.raw_path)?;
        let path = self.module.output(&self.path)?;
        self.last_dimensions = Some((binary.width, binary.height));
        self.binary_snapshot.clear();
        self.binary_snapshot
            .extend_from_slice(&binary.cells[..binary.len]);
        self.cost_snapshot.clear();
        self.cost_snapshot
            .extend_from_slice(&cost.cells[..cost.len]);
        self.raw_snapshot.clear();
        self.raw_snapshot.extend_from_slice(&raw);
        self.path_snapshot.clear();
        self.path_snapshot.extend_from_slice(&path);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct NavigationPostRunSnapshot<'a> {
    pub width: usize,
    pub height: usize,
    pub binary_map: &'a [u8],
    pub cost_map: &'a [u8],
    pub raw_path: &'a [GridPoint],
    pub smoothed_path: Option<&'a [GridPoint]>,
    pub final_path: &'a [GridPoint],
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct NavigationPathMetrics {
    pub points: usize,
    pub length: f64,
    pub turns: usize,
    pub collision_free: bool,
}

impl NavigationPostRunSnapshot<'_> {
    #[must_use]
    pub fn raw_path_metrics(&self) -> NavigationPathMetrics {
        path_metrics(self.width, self.height, self.cost_map, self.raw_path)
    }

    #[must_use]
    pub fn final_path_metrics(&self) -> NavigationPathMetrics {
        path_metrics(self.width, self.height, self.cost_map, self.final_path)
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

impl DecoderUnit {
    fn execute(&mut self, invocation: &RegistrationInvocation<'_>) -> Result<(), RunError> {
        let input = invocation.input_value::<RosOccupancyGrid>(0)?;
        let len = validate_grid(&input, self.max_cells)?;
        let mut map = GridMap {
            width: input.width,
            height: input.height,
            start: input.start,
            goal: input.goal,
            len,
            cells: [0; MAX_CELLS],
        };
        for (target, occupancy) in map.cells[..len].iter_mut().zip(&input.data) {
            *target = u8::from(*occupancy < 0 || *occupancy >= 50);
        }
        invocation.write_value(0, map)
    }
}

impl InflationUnit {
    fn execute(&mut self, invocation: &RegistrationInvocation<'_>) -> Result<(), RunError> {
        let input = invocation.input_value::<GridMap>(0)?;
        if input.len > self.max_cells {
            return Err(RunError::InvalidInput {
                message: "binary map exceeds inflation bound",
            });
        }
        let mut output = (*input).clone();
        if self.radius != 0 {
            for index in 0..input.len {
                if input.cells[index] != 0 {
                    let x = index % input.width;
                    let y = index / input.width;
                    let min_x = x.saturating_sub(self.radius);
                    let max_x = (x + self.radius).min(input.width - 1);
                    let min_y = y.saturating_sub(self.radius);
                    let max_y = (y + self.radius).min(input.height - 1);
                    for iy in min_y..=max_y {
                        for ix in min_x..=max_x {
                            output.cells[iy * input.width + ix] = 1;
                        }
                    }
                }
            }
        }
        invocation.write_value(0, output)
    }
}

impl PlannerUnit {
    fn new(config: &PlannerConfig, algorithm: SearchAlgorithm) -> Result<Self, String> {
        let open_capacity = config
            .max_cells
            .checked_mul(4)
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| "planner open-set bound overflows usize".to_owned())?;
        Ok(Self {
            algorithm,
            max_cells: config.max_cells,
            max_path: config.max_path,
            max_expansions: config.max_expansions,
            distance: vec![INF; config.max_cells],
            parent: vec![usize::MAX; config.max_cells],
            closed: vec![false; config.max_cells],
            open: BinaryHeap::with_capacity(open_capacity),
            raw_path: Vec::with_capacity(config.max_path),
        })
    }

    fn execute(&mut self, invocation: &RegistrationInvocation<'_>) -> Result<(), RunError> {
        let map = invocation.input_value::<GridMap>(0)?;
        if map.len > self.max_cells {
            return Err(RunError::InvalidInput {
                message: "cost map exceeds planner bound",
            });
        }
        self.distance[..map.len].fill(INF);
        self.parent[..map.len].fill(usize::MAX);
        self.closed[..map.len].fill(false);
        self.open.clear();
        self.raw_path.clear();
        let start = usize::from(map.start.y) * map.width + usize::from(map.start.x);
        let goal = usize::from(map.goal.y) * map.width + usize::from(map.goal.x);
        if map.cells[start] != 0 || map.cells[goal] != 0 {
            return Err(RunError::InvalidInput {
                message: "start or goal is occupied after inflation",
            });
        }
        self.distance[start] = 0;
        self.push_open(start, 0, goal, map.width)?;
        let mut expansions = 0;
        while let Some(Reverse((_, best, queued_distance))) = self.open.pop() {
            if self.closed[best] || queued_distance != self.distance[best] {
                continue;
            }
            if best == goal {
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
            let x = best % map.width;
            let y = best / map.width;
            for neighbor in neighbors(x, y, map.width, map.height).into_iter().flatten() {
                if map.cells[neighbor] != 0 || self.closed[neighbor] {
                    continue;
                }
                let candidate = self.distance[best] + 1;
                if candidate < self.distance[neighbor] {
                    self.distance[neighbor] = candidate;
                    self.parent[neighbor] = best;
                    self.push_open(neighbor, candidate, goal, map.width)?;
                }
            }
        }
        if self.distance[goal] != INF {
            let mut current = goal;
            loop {
                if self.raw_path.len() == self.max_path {
                    return Err(capacity("raw_path", self.raw_path.len() + 1, self.max_path));
                }
                self.raw_path.push(GridPoint {
                    x: (current % map.width) as u16,
                    y: (current / map.width) as u16,
                });
                if current == start {
                    break;
                }
                current = self.parent[current];
            }
            self.raw_path.reverse();
        }
        for point in &self.raw_path {
            invocation.push_buffer(0, *point)?;
        }
        Ok(())
    }

    fn push_open(
        &mut self,
        index: usize,
        distance: u32,
        goal: usize,
        width: usize,
    ) -> Result<(), RunError> {
        if self.open.len() == self.open.capacity() {
            return Err(capacity(
                "search_open_set",
                self.open.len() + 1,
                self.open.capacity(),
            ));
        }
        let score = distance.saturating_add(match self.algorithm {
            SearchAlgorithm::Dijkstra => 0,
            SearchAlgorithm::AStar => manhattan(index, goal, width),
        });
        self.open.push(Reverse((score, index, distance)));
        Ok(())
    }
}

impl SmootherUnit {
    fn execute(&mut self, invocation: &RegistrationInvocation<'_>) -> Result<(), RunError> {
        let map = invocation.input_value::<GridMap>(0)?;
        let raw = invocation.input_buffer::<GridPoint>(1)?;
        self.path.clear();
        if !raw.is_empty() {
            let mut anchor = 0;
            self.path.push(raw[0]);
            while anchor + 1 < raw.len() {
                let mut next = raw.len() - 1;
                while next > anchor + 1
                    && !line_is_clear(raw[anchor], raw[next], &map.cells, map.width, map.height)
                {
                    next -= 1;
                }
                if self.path.len() == self.max_path {
                    return Err(capacity(
                        "smoothed_path",
                        self.path.len() + 1,
                        self.max_path,
                    ));
                }
                self.path.push(raw[next]);
                anchor = next;
            }
        }
        for point in &self.path {
            invocation.push_buffer(0, *point)?;
        }
        Ok(())
    }
}

fn validate_grid(input: &RosOccupancyGrid, max_cells: usize) -> Result<usize, RunError> {
    let cells = input
        .width
        .checked_mul(input.height)
        .ok_or(RunError::InvalidInput {
            message: "grid dimensions overflow",
        })?;
    if cells == 0 || cells > max_cells || input.data.len() != cells {
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

pub fn build_from_path(path: &Path) -> Result<PreparedNavigation, String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    build_from_source(&source)
}

pub fn build_from_source(source: &str) -> Result<PreparedNavigation, String> {
    let (units, resources) = registries()?;
    let bounds = BoundSources {
        host: BTreeMap::from([(ResourceId::new("occupancy_grid"), MAX_CELLS)]),
        adapters: BTreeMap::new(),
    };
    let definition = load(source, ParseLimits::default(), &units, &resources, &bounds)
        .map_err(|error| error.to_string())?
        .compile()
        .map_err(|error| error.to_string())?;
    validate_graph(&definition.graph)?;
    let smoothing = definition
        .graph
        .units
        .iter()
        .any(|unit| unit.unit_type.as_str() == "nav.line_of_sight_smoother/v1");
    let storage = plan_storage(&definition.graph, &resources, &definition.requirements)
        .map_err(|error| format!("storage planning failed: {error:?}"))?;
    let configurations = configuration_summaries(&definition)?;
    let prepared = PreparedModuleDescription {
        options: BuildOptions::strict(),
        requirement_status: RequirementStatus::Bounded,
        allocation_capability: AllocationCapability::inspect(
            vec![AllocationDomain {
                name: "rust-global".to_owned(),
                evidence: AllocationEvidence::Instrumented,
            }],
            true,
        ),
        warm_up_is_measured: false,
    };
    let graph = definition.graph.clone();
    let requirements = definition.requirements.clone();
    let workspace_bytes = definition.workspace_bytes.clone();
    let module = Module::build(
        definition.into_executable_definition(),
        &units,
        &resources,
        BuildOptions::strict(),
    )
    .map_err(|error| format!("strict Module build failed: {error:?}"))?;
    let input = module
        .input_handle::<RosOccupancyGrid>(&ResourceId::new("occupancy_grid"))
        .map_err(debug)?;
    let binary_map = module
        .output_handle::<GridMap>(&ResourceId::new("binary_map"))
        .map_err(debug)?;
    let cost_map = module
        .output_handle::<GridMap>(&ResourceId::new("cost_map"))
        .map_err(debug)?;
    let raw_path = module
        .output_handle::<Vec<GridPoint>>(&ResourceId::new("raw_path"))
        .map_err(debug)?;
    let final_resource = if smoothing {
        "smoothed_path"
    } else {
        "raw_path"
    };
    let path = module
        .output_handle::<Vec<GridPoint>>(&ResourceId::new(final_resource))
        .map_err(debug)?;
    let input_plan = PreparedInputPlan::new([PreparedInputSpec::of::<RosOccupancyGrid>(
        ResourceId::new("occupancy_grid"),
        grid_type(),
        MAX_CELLS,
        input.plan_token(),
    )])
    .map_err(|error| format!("input plan failed: {error:?}"))?;
    let description = FixedModuleDescription::new(
        graph.clone(),
        configurations,
        requirements,
        workspace_bytes,
        storage.report().clone(),
        prepared,
    );
    Ok(PreparedNavigation {
        graph,
        description,
        input_plan,
        module,
        input,
        binary_map,
        cost_map,
        raw_path,
        path,
        last_dimensions: None,
        binary_snapshot: Vec::with_capacity(MAX_CELLS),
        cost_snapshot: Vec::with_capacity(MAX_CELLS),
        raw_snapshot: Vec::with_capacity(MAX_PATH),
        path_snapshot: Vec::with_capacity(MAX_PATH),
        smoothing,
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
    let data = DEMO_MAP_ROWS
        .iter()
        .rev()
        .flat_map(|row| row.bytes())
        .map(|cell| match cell {
            b'.' => 0,
            b'#' => 100,
            b'?' => -1,
            _ => unreachable!("demo map contains only '.', '#', and '?'"),
        })
        .collect();

    RosOccupancyGrid {
        width: DEMO_WIDTH,
        height: DEMO_HEIGHT,
        data,
        start: GridPoint { x: 18, y: 9 },
        goal: GridPoint { x: 35, y: 29 },
    }
}

/// Builds the fixed 1,000-leg workload used by the visualization commands.
pub fn demo_itinerary() -> NavigationItinerary {
    let grid = demo_grid();
    let free = inflated_free_cells(&grid, 1);
    let mut current = grid.start;
    let mut legs = Vec::with_capacity(EPISODE_LEGS);
    let mut counts = [0_usize; 3];

    for index in 0..EPISODE_LEGS {
        let bucket = match index % 3 {
            0 => RouteDistanceBucket::Short,
            1 => RouteDistanceBucket::Medium,
            _ => RouteDistanceBucket::Long,
        };
        let distances = route_distances(&free, grid.width, grid.height, current);
        let candidates = distances
            .iter()
            .enumerate()
            .filter_map(|(cell, &distance)| {
                let matches = match bucket {
                    RouteDistanceBucket::Short => (3..=6).contains(&distance),
                    RouteDistanceBucket::Medium => (10..=16).contains(&distance),
                    RouteDistanceBucket::Long => (20..=MAX_PATH - 1).contains(&distance),
                };
                matches.then_some((cell, distance))
            })
            .collect::<Vec<_>>();
        assert!(
            !candidates.is_empty(),
            "fixture has no candidate for {bucket:?}"
        );
        let (goal_cell, _) = candidates[(index * 37 + 11) % candidates.len()];
        let goal = GridPoint {
            x: u16::try_from(goal_cell % grid.width).expect("fixture width fits u16"),
            y: u16::try_from(goal_cell / grid.width).expect("fixture height fits u16"),
        };
        legs.push(NavigationLeg {
            start: current,
            goal,
        });
        counts[bucket_index(bucket)] += 1;
        current = goal;
    }

    assert_eq!(counts, [334, 333, 333]);
    assert!(legs.windows(2).all(|pair| pair[0].goal == pair[1].start));
    NavigationItinerary { legs }
}

fn bucket_index(bucket: RouteDistanceBucket) -> usize {
    match bucket {
        RouteDistanceBucket::Short => 0,
        RouteDistanceBucket::Medium => 1,
        RouteDistanceBucket::Long => 2,
    }
}

fn inflated_free_cells(grid: &RosOccupancyGrid, radius: usize) -> Vec<bool> {
    let mut free = vec![false; grid.data.len()];
    for y in 0..grid.height {
        for x in 0..grid.width {
            let clear = (y.saturating_sub(radius)..=(y + radius).min(grid.height - 1)).all(|ny| {
                (x.saturating_sub(radius)..=(x + radius).min(grid.width - 1))
                    .all(|nx| grid.data[ny * grid.width + nx] == 0)
            });
            free[y * grid.width + x] = clear;
        }
    }
    free
}

fn route_distances(free: &[bool], width: usize, height: usize, start: GridPoint) -> Vec<usize> {
    let mut distances = vec![usize::MAX; free.len()];
    let start = usize::from(start.y) * width + usize::from(start.x);
    assert!(
        free[start],
        "itinerary endpoint is outside inflated free space"
    );
    distances[start] = 0;
    let mut queue = VecDeque::from([start]);
    while let Some(cell) = queue.pop_front() {
        let x = cell % width;
        let y = cell / width;
        for next in [
            x.checked_sub(1).map(|nx| y * width + nx),
            (x + 1 < width).then_some(y * width + x + 1),
            y.checked_sub(1).map(|ny| ny * width + x),
            (y + 1 < height).then_some((y + 1) * width + x),
        ]
        .into_iter()
        .flatten()
        {
            if free[next] && distances[next] == usize::MAX {
                distances[next] = distances[cell] + 1;
                queue.push_back(next);
            }
        }
    }
    distances
}

fn registries() -> Result<(UnitRegistry, ResourceRegistry), String> {
    let grid = grid_type();
    let map = semantic("nav.BinaryMap/v1")?;
    let path = semantic("nav.Path/v1")?;
    let mut resources = ResourceRegistry::default();
    resources
        .register(ResourceDescriptor::of::<RosOccupancyGrid>(
            grid.clone(),
            "bounded ROS occupancy-grid view",
            "dimensions and data length agree with host bound",
        ))
        .map_err(debug)?;
    resources
        .register(ResourceDescriptor::of::<GridMap>(
            map.clone(),
            "fixed bounded grid-map value",
            "metadata and initialized cells agree",
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
    let mut units = UnitRegistry::default();
    register_unit::<DecoderConfig, _>(
        &mut units,
        "nav.ros_map_decoder/v1",
        vec![port::<RosOccupancyGrid>("grid", &grid)],
        vec![port::<GridMap>("map", &map)],
        |config, _| Ok(requirement("map", 1, config.max_cells)),
    )?;
    let decoder_type = UnitTypeName::new("nav.ros_map_decoder/v1");
    units
        .register_factory::<DecoderConfig, DecoderUnit, _>(&decoder_type, |config| {
            Ok(DecoderUnit {
                max_cells: config.max_cells,
            })
        })
        .map_err(debug)?;
    units
        .register_executor::<DecoderUnit, _>(&decoder_type, |unit, invocation, _| {
            unit.execute(invocation)
        })
        .map_err(debug)?;
    register_unit::<InflationConfig, _>(
        &mut units,
        "nav.binary_inflation/v1",
        vec![port::<GridMap>("map", &map)],
        vec![port::<GridMap>("cost_map", &map)],
        |config, _| Ok(requirement("cost_map", 1, config.max_cells)),
    )?;
    let inflation_type = UnitTypeName::new("nav.binary_inflation/v1");
    units
        .register_factory::<InflationConfig, InflationUnit, _>(&inflation_type, |config| {
            Ok(InflationUnit {
                radius: config.radius,
                max_cells: config.max_cells,
            })
        })
        .map_err(debug)?;
    units
        .register_executor::<InflationUnit, _>(&inflation_type, |unit, invocation, _| {
            unit.execute(invocation)
        })
        .map_err(debug)?;
    for planner in ["nav.astar/v1", "nav.dijkstra/v1"] {
        register_unit::<PlannerConfig, _>(
            &mut units,
            planner,
            vec![port::<GridMap>("cost_map", &map)],
            vec![port::<Vec<GridPoint>>("path", &path)],
            |config, _| {
                Ok(requirement(
                    "path",
                    config.max_path,
                    config.max_cells * (size_of::<u32>() + size_of::<usize>() + 1),
                ))
            },
        )?;
        let planner_type = UnitTypeName::new(planner);
        let algorithm = if planner == "nav.astar/v1" {
            SearchAlgorithm::AStar
        } else {
            SearchAlgorithm::Dijkstra
        };
        units
            .register_factory::<PlannerConfig, PlannerUnit, _>(&planner_type, move |config| {
                PlannerUnit::new(config, algorithm)
            })
            .map_err(debug)?;
        units
            .register_executor::<PlannerUnit, _>(&planner_type, |unit, invocation, _| {
                unit.execute(invocation)
            })
            .map_err(debug)?;
    }
    register_unit::<SmootherConfig, _>(
        &mut units,
        "nav.line_of_sight_smoother/v1",
        vec![
            port::<GridMap>("cost_map", &map),
            port::<Vec<GridPoint>>("path", &path),
        ],
        vec![port::<Vec<GridPoint>>("path", &path)],
        |config, _| Ok(requirement("path", config.max_path, 0)),
    )?;
    let smoother_type = UnitTypeName::new("nav.line_of_sight_smoother/v1");
    units
        .register_factory::<SmootherConfig, SmootherUnit, _>(&smoother_type, |config| {
            Ok(SmootherUnit {
                max_path: config.max_path,
                path: Vec::with_capacity(config.max_path),
            })
        })
        .map_err(debug)?;
    units
        .register_executor::<SmootherUnit, _>(&smoother_type, |unit, invocation, _| {
            unit.execute(invocation)
        })
        .map_err(debug)?;
    Ok((units, resources))
}

fn validate_graph(graph: &CompiledGraph) -> Result<(), String> {
    let cost_map = graph
        .resources
        .iter()
        .find(|resource| resource.id.as_str() == "cost_map")
        .ok_or_else(|| "graph has no cost_map Resource".to_owned())?;
    for required in ["binary_map", "cost_map", "raw_path"] {
        if !graph
            .module_outputs
            .iter()
            .any(|resource| resource.as_str() == required)
        {
            return Err(format!("navigation graph must publish {required}"));
        }
    }
    let mut consumers: Vec<_> = cost_map
        .consumers
        .iter()
        .map(|consumer| consumer.unit.as_str())
        .collect();
    consumers.sort_unstable();
    let smoothing = graph.units.iter().any(|unit| unit.id.as_str() == "smooth");
    let expected = if smoothing {
        vec!["plan", "smooth"]
    } else {
        vec!["plan"]
    };
    if consumers != expected {
        return Err(format!(
            "cost_map consumers must be {expected:?}, found {consumers:?}"
        ));
    }
    Ok(())
}

fn register_unit<T, F>(
    units: &mut UnitRegistry,
    name: &str,
    inputs: Vec<PortDescriptor>,
    outputs: Vec<PortDescriptor>,
    requirements: F,
) -> Result<(), String>
where
    T: serde::de::DeserializeOwned + Send + Sync + 'static,
    F: Fn(&T, &BoundSources) -> Result<UnitRequirements, String> + 'static,
{
    let unit_type = UnitTypeName::new(name);
    register_yaml_unit::<T, _>(
        units,
        UnitDescriptor {
            type_name: unit_type.clone(),
            inputs,
            outputs,
        },
        requirements,
    )
    .map_err(debug)?;
    units
        .set_allocation_capability(
            &unit_type,
            AllocationCapability::inspect(
                vec![AllocationDomain {
                    name: "rust-global".to_owned(),
                    evidence: AllocationEvidence::Instrumented,
                }],
                true,
            ),
        )
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

fn path_metrics(
    width: usize,
    height: usize,
    cost_map: &[u8],
    path: &[GridPoint],
) -> NavigationPathMetrics {
    let length = path
        .windows(2)
        .map(|points| {
            let dx = f64::from(points[1].x) - f64::from(points[0].x);
            let dy = f64::from(points[1].y) - f64::from(points[0].y);
            dx.hypot(dy)
        })
        .sum();
    let turns = path
        .windows(3)
        .filter(|points| {
            let first_x = i64::from(points[1].x) - i64::from(points[0].x);
            let first_y = i64::from(points[1].y) - i64::from(points[0].y);
            let second_x = i64::from(points[2].x) - i64::from(points[1].x);
            let second_y = i64::from(points[2].y) - i64::from(points[1].y);
            first_x * second_y != first_y * second_x
        })
        .count();
    let collision_free = !path.is_empty()
        && path
            .windows(2)
            .all(|points| line_is_clear(points[0], points[1], cost_map, width, height));
    NavigationPathMetrics {
        points: path.len(),
        length,
        turns,
        collision_free,
    }
}

#[cfg(test)]
mod itinerary_tests {
    use super::{EPISODE_LEGS, demo_itinerary};

    #[test]
    fn demo_episode_is_deterministic_chained_and_bucketed() {
        let itinerary = demo_itinerary();
        assert_eq!(itinerary.legs.len(), EPISODE_LEGS);
        assert!(
            itinerary
                .legs
                .windows(2)
                .all(|pair| pair[0].goal == pair[1].start)
        );
    }
}
