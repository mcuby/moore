initSidebarItems({"macro":[["debugln","Print debug information. Omitted in release builds."],["make_arenas","Generate a collection of arenas for different types."],["node_ref","Create a new node reference."],["node_ref_group","Create a new group of node references."],["node_storage","Create a new table that implements the `NodeStorage` trait."]],"mod":[["arenas","Multi-type arena allocation"],["errors","Utilities to implement diagnostics and error reporting facilities."],["grind","This module provides an abstraction similar to iterators. Elements are produced in one direction, while errors bubble backwards until they are vented. This allows for complex transformation chains such as lexical analyzers and parsers to be constructed, where errors, warnings, or notices might be emitted without disturbing the transformation."],["id",""],["lexer","Lexer utilities."],["name","A name table that internalizes all names presented to it and allows for them to be referred to by a lightweight tag. This structure is heavily inspired by the interner used in the Rust compiler."],["score","This module implements the scoreboard building blocks. Scoreboards are the driving mechanism behind moore. They keep track of the results of each compilation step for every node in the graph. Each node can be accessed in a type safe manner by its ID."],["source","A global source file table that assigns an opaque ID to each processed source file. This helps keeping the source location lean and allow for simple querying of information."],["util","A collection of utility traits and functions specific to VHDL."]],"struct":[["Session",""],["SessionOptions","A set of options for a session."],["Verbosity","A set of verbosity options for a session."]],"trait":[["SessionContext","Access session options and emit diagnostics."]]});