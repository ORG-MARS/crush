use crate::lang::command::{Command, CrushCommand, OutputType};
use crate::lang::errors::{error, mandate, CrushResult};
use crate::lang::execution_context::CommandContext;
use crate::lang::help::Help;
use crate::lang::data::r#struct::Struct;
use crate::lang::{value::Value, value::ValueType};
use crate::util::identity_arc::Identity;
use ordered_map::OrderedMap;
use std::cmp::max;
use std::sync::{Arc, Mutex, MutexGuard};
use std::fmt::{Display, Formatter};

/**
  This is where we store variables, including functions.

  The Scope type is both used to implement namespaces and as the function stack.

  The data is protected by a mutex, in order to make sure that all threads can read and write
  concurrently.

  The data is protected by an Arc, in order to make sure that it gets deallocated and can be shared
  across threads.

  In order to ensure that there are no deadlocks, a given thread will only ever lock one scope at a
  time. This forces us to manually drop some variables making some of the code in this file look a
  little wonky and cumbersome.
*/
#[derive(Clone)]
pub struct Scope {
    data: Arc<Mutex<ScopeData>>,
}

/**
    The ScopeLoader type allows us to lazy-load namespaces.
    Without it, every single library in Crush would be loaded on startup.
*/
pub struct ScopeLoader {
    mapping: OrderedMap<String, Value>,
    path: Vec<String>,
    parent: Scope,
    scope: Scope,
}

impl ScopeLoader {
    pub fn declare(&mut self, name: &str, value: Value) -> CrushResult<()> {
        if self.mapping.contains_key(name) {
            return error(format!("Variable {{{}}} already exists", name).as_str());
        }
        self.mapping.insert(name.to_string(), value);
        Ok(())
    }

    /**
        Create a namespace. Namespaces are lazily loaded, so on creating, only a stub is created,
        and the first time a namespace is used, the loader function will be called, and that will
        load the namespace.
    */
    pub fn create_namespace(
        &mut self,
        name: &str,
        loader: Box<dyn Send + FnOnce(&mut ScopeLoader) -> CrushResult<()>>,
    ) -> CrushResult<Scope> {
        let res = Scope {
            data: Arc::from(Mutex::new(ScopeData::lazy(
                None,
                Some(self.scope.clone()),
                false,
                Some(name.to_string()),
                loader,
            ))),
        };
        self.declare(name, Value::Scope(res.clone()))?;
        Ok(res)
    }

    pub fn declare_command(
        &mut self,
        name: &str,
        call: fn(CommandContext) -> CrushResult<()>,
        can_block: bool,
        signature: &'static str,
        short_help: &'static str,
        long_help: Option<&'static str>,
        output: OutputType,
    ) -> CrushResult<()> {
        let mut full_name = self.path.clone();
        full_name.push(name.to_string());
        let command = CrushCommand::command(
            call, can_block, full_name, signature, short_help, long_help, output,
        );
        if self.mapping.contains_key(name) {
            return error(format!("Variable {{{}}} already exists", name).as_str());
        }
        self.mapping
            .insert(name.to_string(), Value::Command(command));
        Ok(())
    }

    pub fn declare_condition_command(
        &mut self,
        name: &str,
        call: fn(context: CommandContext) -> CrushResult<()>,
        signature: &'static str,
        short_help: &'static str,
        long_help: Option<&'static str>,
    ) -> CrushResult<()> {
        let mut full_name = self.path.clone();
        full_name.push(name.to_string());
        let command = CrushCommand::condition(call, full_name, signature, short_help, long_help);
        if self.mapping.contains_key(name) {
            return error(format!("Variable {{{}}} already exists", name).as_str());
        }
        self.mapping
            .insert(name.to_string(), Value::Command(command));
        Ok(())
    }

    fn copy_into(&mut self, target: &mut OrderedMap<String, Value>) {
        for (k, v) in self.mapping.drain() {
            target.insert(k, v);
        }
    }

    pub fn create_temporary_namespace(&self) -> Scope {
        Scope {
            data: Arc::from(Mutex::new(ScopeData::new(
                Some(self.parent.clone()),
                Some(self.parent.clone()),
                false,
                None,
            ))),
        }
    }
}

pub struct ScopeData {
    /** This is the parent scope used to perform variable name resolution. If a variable lookup
    fails in the current scope, it proceeds to this scope. This is usually the scope in which this
    scope was *created*.

    Not that when scopes are used as namespaces, they do not use this scope.
    */
    pub parent_scope: Option<Scope>,

    /** This is the scope in which the current scope was called. Since a closure can be called
    from inside any scope, it need not be the same as the parent scope. This scope is the one used
    for break/continue loop control, and it is also the scope that builds up the namespace hierarchy. */
    pub calling_scope: Option<Scope>,

    /** This is a list of scopes that are imported into the current scope. Anything directly inside
    one of these scopes is also considered part of this scope. */
    pub uses: Vec<Scope>,

    /** The actual data of this scope. */
    pub mapping: OrderedMap<String, Value>,

    /** True if this scope is a loop. Required to implement the break/continue commands.*/
    pub is_loop: bool,

    /** True if this scope should stop execution, i.e. if the continue or break commands have been
    called.  */
    pub is_stopped: bool,

    /** True if this scope can not be further modified. Note that mutable variables in it, e.g.
    lists can still be modified. */
    pub is_readonly: bool,

    pub name: Option<String>,
    is_loaded: bool,
    loader: Option<Box<dyn Send + FnOnce(&mut ScopeLoader) -> CrushResult<()>>>,
}

impl ScopeData {
    fn new(
        parent_scope: Option<Scope>,
        calling_scope: Option<Scope>,
        is_loop: bool,
        name: Option<String>,
    ) -> ScopeData {
        ScopeData {
            parent_scope,
            calling_scope,
            is_loop,
            uses: Vec::new(),
            mapping: OrderedMap::new(),
            is_stopped: false,
            is_readonly: false,
            name,
            is_loaded: true,
            loader: None,
        }
    }

    fn lazy(
        parent_scope: Option<Scope>,
        calling_scope: Option<Scope>,
        is_loop: bool,
        name: Option<String>,
        loader: Box<dyn Send + FnOnce(&mut ScopeLoader) -> CrushResult<()>>,
    ) -> ScopeData {
        ScopeData {
            parent_scope,
            calling_scope,
            is_loop,
            uses: Vec::new(),
            mapping: OrderedMap::new(),
            is_stopped: false,
            is_readonly: false,
            name,
            is_loaded: false,
            loader: Some(loader),
        }
    }
}

impl Clone for ScopeData {
    fn clone(&self) -> Self {
        ScopeData {
            parent_scope: self.parent_scope.clone(),
            calling_scope: self.calling_scope.clone(),
            is_loop: self.is_loop,
            uses: self.uses.clone(),
            mapping: self.mapping.clone(),
            is_stopped: self.is_stopped,
            is_readonly: self.is_readonly,
            name: self.name.clone(),
            is_loaded: true,
            loader: None,
        }
    }
}

impl Scope {
    pub fn create_root() -> Scope {
        Scope {
            data: Arc::from(Mutex::new(ScopeData::new(
                None,
                None,
                false,
                Some("global".to_string()),
            ))),
        }
    }

    pub fn create(
        name: Option<String>,
        is_loop: bool,
        is_stopped: bool,
        is_readonly: bool,
    ) -> Scope {
        Scope {
            data: Arc::from(Mutex::new(ScopeData {
                parent_scope: None,
                calling_scope: None,
                uses: vec![],
                mapping: OrderedMap::new(),
                is_loop,
                is_stopped,
                is_readonly,
                name,
                is_loaded: true,
                loader: None,
            })),
        }
    }

    pub fn create_child(&self, caller: &Scope, is_loop: bool) -> Scope {
        Scope {
            data: Arc::from(Mutex::new(ScopeData::new(
                Some(self.clone()),
                Some(caller.clone()),
                is_loop,
                None,
            ))),
        }
    }

    pub fn create_namespace(
        &self,
        name: &str,
        loader: Box<dyn Send + FnOnce(&mut ScopeLoader) -> CrushResult<()>>,
    ) -> CrushResult<Scope> {
        let res = Scope {
            data: Arc::from(Mutex::new(ScopeData::lazy(
                None,
                Some(self.clone()),
                false,
                Some(name.to_string()),
                loader,
            ))),
        };
        self.declare(name, Value::Scope(res.clone()))?;
        Ok(res)
    }

    pub fn do_continue(&self) -> CrushResult<bool> {
        let data = self.lock()?;
        if data.is_readonly {
            Ok(false)
        } else if data.is_loop {
            Ok(true)
        } else {
            let caller = data.calling_scope.clone();
            drop(data);
            let ok = caller.map(|p| p.do_continue()).unwrap_or(Ok(false))?;
            if !ok {
                Ok(false)
            } else {
                self.lock().unwrap().is_stopped = true;
                Ok(true)
            }
        }
    }

    pub fn do_break(&self) -> CrushResult<bool> {
        let mut data = self.lock()?;
        if data.is_readonly {
            Ok(false)
        } else if data.is_loop {
            data.is_stopped = true;
            Ok(true)
        } else {
            let caller = data.calling_scope.clone();
            drop(data);
            let ok = caller.map(|p| p.do_break()).unwrap_or(Ok(false))?;
            if !ok {
                Ok(false)
            } else {
                self.lock().unwrap().is_stopped = true;
                Ok(true)
            }
        }
    }

    pub fn is_stopped(&self) -> bool {
        self.lock().unwrap().is_stopped
    }

    fn lock(&self) -> CrushResult<MutexGuard<ScopeData>> {
        let mut data = self.data.lock().unwrap();
        if data.is_loaded {
            return Ok(data);
        }

        drop(data);
        let path = self.full_path()?;

        data = self.data.lock().unwrap();
        if data.is_loaded {
            return Ok(data);
        }
        data.is_loaded = true;
        let loader = mandate(data.loader.take(), "Missing module loader")?;
        let mut tmp = ScopeLoader {
            mapping: OrderedMap::new(),
            path,
            parent: data.calling_scope.as_ref().unwrap().clone(),
            scope: self.clone(),
        };
        loader(&mut tmp)?;
        tmp.copy_into(&mut data.mapping);
        data.is_readonly = true;

        Ok(data)
    }

    pub fn clear(&self) {
        let mut data = self.lock().unwrap();
        data.mapping.clear();
        data.uses.clear();
    }

    pub fn full_path(&self) -> CrushResult<Vec<String>> {
        let data = self.data.lock().unwrap();
        match data.name.clone() {
            None => error("Tried to get full path of anonymous scope"),
            Some(name) => match data.calling_scope.clone() {
                None => Ok(vec![name]),
                Some(parent) => {
                    drop(data);
                    let mut full_path = parent.full_path()?;
                    full_path.push(name);
                    Ok(full_path)
                }
            },
        }
    }

    /**
        Returns the "root" object, which is the object that classes inherit from.
    */
    pub fn root_object(&self) -> Struct {
        match self.get_absolute_path(vec![
            "global".to_string(),
            "types".to_string(),
            "root".to_string(),
        ]) {
            Ok(Value::Struct(s)) => s,
            _ => panic!("Root missing!"),
        }
    }

    pub fn global_static_cmd(&self, full_path: Vec<&str>) -> CrushResult<Command> {
        match self.get_absolute_path(full_path.iter().map(|p| p.to_string()).collect()) {
            Ok(Value::Command(cmd)) => Ok(cmd),
            Err(e) => Err(e),
            _ => error("Expected a command"),
        }
    }

    /**
        Resolve the given path from the root of the namespace
    */
    pub fn get_absolute_path(&self, absolute_path: Vec<String>) -> CrushResult<Value> {
        let data = self.lock()?;
        match data.calling_scope.clone() {
            Some(parent) => {
                drop(data);
                parent.get_absolute_path(absolute_path)
            }
            None => {
                drop(data);
                self.get_recursive(&absolute_path[..])
            }
        }
    }

    fn get_recursive(&self, path: &[String]) -> CrushResult<Value> {
        if path.is_empty() {
            error("Invalid path")
        } else {
            let data = self.lock()?;
            match data.name.clone() {
                None => error("Anonymous scope!"),
                Some(name) => {
                    if name != path[0] {
                        error(format!(
                            "Invalid scope during lookup, expected scope named {}, found one named {}",
                            path[0], name).as_str())
                    } else {
                        match path.len() {
                            1 => Ok(Value::Scope(self.clone())),
                            2 => match data.mapping.get(&path[1]) {
                                Some(v) => Ok(v.clone()),
                                _ => error(
                                    format!(
                                        "Could not find command {} in scope {}",
                                        path[1], path[0]
                                    )
                                        .as_str(),
                                ),
                            },
                            _ => {
                                let s = data.mapping.get(&path[1]).map(Value::clone);
                                drop(data);
                                match s {
                                    Some(Value::Scope(s)) => s.get_recursive(&path[1..]),
                                    Some(v) => v.get_recursive(&path[1..]),
                                    _ => {
                                        let data = self.lock()?;
                                        error(
                                            format!(
                                                "Could not find subscope {} in scope {} {}. Candidates are {}",
                                                path.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(":"),
                                                //path[1],
                                                 data.name.clone().unwrap(),
                                                self.id(),
                                                data.mapping.iter().map(|(k, _)| k.to_string()).collect::<Vec<_>>().join(", "),
                                            )
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn declare(&self, name: &str, value: Value) -> CrushResult<()> {
        let mut data = self.lock()?;
        if data.is_readonly {
            return error("Scope is read only");
        }
        if data.mapping.contains_key(name) {
            return error(format!("Variable {{{}}} already exists", name).as_str());
        }
        data.mapping.insert(name.to_string(), value);
        Ok(())
    }

    pub fn redeclare(&self, name: &str, value: Value) -> CrushResult<()> {
        let mut data = self.lock()?;
        if data.is_readonly {
            return error("Scope is read only");
        }
        data.mapping.insert(name.to_string(), value);
        Ok(())
    }

    pub fn set(&self, name: &str, value: Value) -> CrushResult<()> {
        let mut data = self.lock()?;
        if !data.mapping.contains_key(name) {
            match data.parent_scope.clone() {
                Some(p) => {
                    drop(data);
                    p.set(name, value)
                }
                None => error(format!("Unknown variable {}", name).as_str()),
            }
        } else if data.is_readonly {
            error(format!("Tried to modify {}, a member of a read-only scope", name))
        } else if data.mapping[name].value_type() != value.value_type() {
            error(format!("Type mismatch when reassigning variable {{{}}}. Use `var:unset \"{}\"` to remove old variable.", name, name).as_str())
        } else {
            data.mapping.insert(name.to_string(), value);
            Ok(())
        }
    }

    pub fn remove_str(&self, name: &str) -> CrushResult<Option<Value>> {
        let n = &name
            .split(':')
            .map(|s| s.to_string())
            .collect::<Vec<String>>()[..];
        self.remove(n)
    }

    pub fn remove(&self, name: &[String]) -> CrushResult<Option<Value>> {
        if name.is_empty() {
            return Ok(None);
        }
        if name.len() == 1 {
            self.remove_here(name[0].as_ref())
        } else {
            match self.get(name[0].as_ref())? {
                None => Ok(None),
                Some(Value::Scope(env)) => env.remove(&name[1..name.len()]),
                _ => Ok(None),
            }
        }
    }

    fn remove_here(&self, key: &str) -> CrushResult<Option<Value>> {
        let mut data = self.lock()?;
        if !data.mapping.contains_key(key) {
            match data.parent_scope.clone() {
                Some(p) => {
                    drop(data);
                    p.remove_here(key)
                }
                None => Ok(None),
            }
        } else {
            if data.is_readonly {
                return Ok(None);
            }
            Ok(data.mapping.remove(key))
        }
    }

    pub fn get(&self, name: &str) -> CrushResult<Option<Value>> {
        let data = self.lock()?;
        match data.mapping.get(name) {
            Some(v) => Ok(Some(v.clone())),
            None => {
                let uses = data.uses.clone();
                drop(data);
                for used in &uses {
                    if let Some(res) = used.get(name)? {
                        return Ok(Some(res));
                    }
                }

                let data2 = self.lock()?;

                match data2.parent_scope.clone() {
                    Some(p) => {
                        drop(data2);
                        p.get(name)
                    }
                    None => Ok(None),
                }
            }
        }
    }

    pub fn r#use(&self, other: &Scope) {
        self.lock().unwrap().uses.push(other.clone());
    }

    pub fn dump(&self, map: &mut OrderedMap<String, ValueType>) -> CrushResult<()> {
        if let Some(p) = self.lock()?.parent_scope.clone() {
            p.dump(map)?;
        }

        for u in self.lock().unwrap().uses.clone().iter().rev() {
            u.dump(map)?;
        }

        let data = self.lock()?;
        for (k, v) in data.mapping.iter() {
            map.insert(k.to_string(), v.value_type());
        }
        Ok(())
    }

    pub fn readonly(&self) {
        self.lock().unwrap().is_readonly = true;
    }

    pub fn export(&self) -> CrushResult<ScopeData> {
        Ok(self.lock()?.clone())
    }

    pub fn set_parent(&self, parent: Option<Scope>) {
        self.lock().unwrap().parent_scope = parent;
    }

    pub fn set_calling(&self, calling: Option<Scope>) {
        self.lock().unwrap().calling_scope = calling;
    }
}

impl Display for Scope {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut map = OrderedMap::new();
        if let Err(_) = self.dump(&mut map) {
            return Err(std::fmt::Error {});
        }

        let mut first = true;
        for (key, _) in map.iter() {
            if first {
                first = false;
            } else {
                f.write_str(", ")?;
            }
            f.write_str(&key)?;
        }
        Ok(())
    }
}

impl Identity for Scope {
    fn id(&self) -> u64 {
        self.data.id()
    }
}

impl Help for Scope {
    fn signature(&self) -> String {
        self.full_path()
            .map(|p| p.join(":"))
            .unwrap_or_else(|_| "<Anonymous scope>".to_string())
    }

    fn short_help(&self) -> String {
        "A namespace".to_string()
    }

    fn long_help(&self) -> Option<String> {
        let mut lines = Vec::new();

        let data = self.lock().unwrap();
        let mut keys: Vec<_> = data.mapping.iter().collect();
        keys.sort_by(|x, y| x.0.cmp(&y.0));

        long_help_methods(&mut keys, &mut lines);
        Some(lines.join("\n"))
    }
}

fn long_help_methods(fields: &mut Vec<(&String, &Value)>, lines: &mut Vec<String>) {
    let mut max_len = 0;
    for (k, _) in fields.iter() {
        max_len = max(max_len, k.len());
    }
    for (k, v) in fields.drain(..) {
        lines.push(format!(
            "    * {}  {}{}",
            k,
            " ".repeat(max_len - k.len()),
            v.short_help()
        ));
    }
}
