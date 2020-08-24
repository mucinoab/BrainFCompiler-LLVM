use std::{collections::VecDeque, env, fs, process::Command};

use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Linkage;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::values::{FunctionValue, PointerValue};
use inkwell::{AddressSpace, IntPredicate, OptimizationLevel};

struct WhileBlock<'a> {
    while_start: BasicBlock<'a>,
    while_body: BasicBlock<'a>,
    while_end: BasicBlock<'a>,
}

struct Types<'a> {
    i64: inkwell::types::IntType<'a>,
    i32: inkwell::types::IntType<'a>,
    //i8: inkwell::types::IntType<'a>,
    //context: &'a inkwell::context::Context,
    i8_ptr: inkwell::types::PointerType<'a>,
}

impl<'a> Types<'a> {
    fn new(context: &'a Context) -> Types<'a> {
        Types {
            i64: context.i64_type(),
            i32: context.i32_type(),
            //i8: context.i8_type(),
            i8_ptr: context.i8_type().ptr_type(AddressSpace::Generic),
            //context,
        }
    }
}

fn main() {
    let source = env::args().nth(1).expect("No source file name provided.");
    let program = fs::read_to_string(&source).expect("Source file not found.");
    let source = &source.trim_end_matches(".bf");

    let mut file_iterator = program.chars().peekable();
    let mut cola = VecDeque::with_capacity(4096);
    let mut consecutive = 1;

    let context = Context::create();
    let module = context.create_module("main");
    let builder = context.create_builder();
    let types = Types::new(&context);

    let calloc_fn_type = types
        .i8_ptr
        .fn_type(&[types.i64.into(), types.i64.into()], false);
    let calloc_fn = module.add_function("calloc", calloc_fn_type, Some(Linkage::External));

    let getchar_fn_type = types.i32.fn_type(&[], false);
    let getchar_fn = module.add_function("getchar", getchar_fn_type, Some(Linkage::External));

    let putchar_fn_type = types.i32.fn_type(&[types.i32.into()], false);
    let putchar_fn = module.add_function("putchar", putchar_fn_type, Some(Linkage::External));

    let main_fn_type = types.i32.fn_type(&[], false);
    let main_fn = module.add_function("main", main_fn_type, Some(Linkage::External));

    builder.position_at_end(context.append_basic_block(main_fn, "entry"));

    let data = builder.build_alloca(types.i8_ptr, "data");
    let ptr = builder.build_alloca(types.i8_ptr, "ptr");

    let data_ptr = builder.build_call(
        calloc_fn,
        &[
            types.i64.const_int(30_000, false).into(),
            types.i64.const_int(1, false).into(),
        ],
        "calloc_call",
    );

    let data_ptr_basic_val = data_ptr.try_as_basic_value().left().unwrap();

    builder.build_store(data, data_ptr_basic_val);
    builder.build_store(ptr, data_ptr_basic_val);

    while let Some(command) = file_iterator.next() {
        match command {
            '>' => {
                while file_iterator.peek() == Some(&'>') {
                    consecutive += 1;
                    file_iterator.next();
                }

                build_add_ptr(&context, &builder, consecutive, &ptr);
            }

            '<' => {
                while file_iterator.peek() == Some(&'<') {
                    consecutive += 1;
                    file_iterator.next();
                }

                build_add_ptr(&context, &builder, -consecutive, &ptr);
            }

            '+' => {
                while file_iterator.peek() == Some(&'+') {
                    consecutive += 1;
                    file_iterator.next();
                }

                build_add(&context, &builder, consecutive, &ptr);
            }

            '-' => {
                while file_iterator.peek() == Some(&'-') {
                    consecutive += 1;
                    file_iterator.next();
                }

                build_add(&context, &builder, -consecutive, &ptr);
            }

            ',' => build_get(&context, &builder, &getchar_fn, &ptr),

            '.' => build_put(&context, &builder, &putchar_fn, &ptr),

            '[' => build_while_start(&context, &builder, &main_fn, &ptr, &mut cola),

            ']' => build_while_end(&builder, &mut cola),

            _ => {}
        }
        consecutive = 1;
    }

    builder.build_return(Some(&types.i32.const_int(0, false)));

    Target::initialize_all(&InitializationConfig::default());
    let target_triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&target_triple).unwrap();

    let target_machine = target
        .create_target_machine(
            &target_triple,
            &TargetMachine::get_host_cpu_name().to_string(),
            &TargetMachine::get_host_cpu_features().to_string(),
            OptimizationLevel::Aggressive,
            RelocMode::Default,
            CodeModel::Default,
        )
        .expect("Generating target machine.");

    target_machine
        .write_to_file(&module, FileType::Object, source.as_ref())
        .expect("Writing EFL File.");

    Command::new("clang")
        .args(&[source, "-Ofast"])
        .status()
        .expect("Linking.");

    Command::new("./a.out")
        .status()
        .expect("Running generated program.");
}

fn build_add_ptr(context: &Context, builder: &Builder, amount: i64, ptr: &PointerValue) {
    let i32_amount = context.i32_type().const_int(amount as u64, false);
    let ptr_load = builder.build_load(*ptr, "load ptr").into_pointer_value();
    let result = unsafe { builder.build_in_bounds_gep(ptr_load, &[i32_amount], "add to pointer") };
    builder.build_store(*ptr, result);
}

fn build_add(context: &Context, builder: &Builder, amount: i64, ptr: &PointerValue) {
    let i8_amount = context.i8_type().const_int(amount as u64, false);
    let ptr_load = builder.build_load(*ptr, "load ptr").into_pointer_value();
    let ptr_val = builder.build_load(ptr_load, "load ptr value");
    let result = builder.build_int_add(ptr_val.into_int_value(), i8_amount, "add to data ptr");
    builder.build_store(ptr_load, result);
}

fn build_get(context: &Context, builder: &Builder, getchar_fn: &FunctionValue, ptr: &PointerValue) {
    let getchar_call = builder.build_call(*getchar_fn, &[], "getchar call");
    let getchar_basicvalue = getchar_call.try_as_basic_value().left().unwrap();

    let truncated = builder.build_int_truncate(
        getchar_basicvalue.into_int_value(),
        context.i8_type(),
        "getchar truncate result",
    );

    let ptr_value = builder
        .build_load(*ptr, "load ptr value")
        .into_pointer_value();
    builder.build_store(ptr_value, truncated);
}

fn build_put(context: &Context, builder: &Builder, putchar_fn: &FunctionValue, ptr: &PointerValue) {
    let char_to_put = builder.build_load(
        builder
            .build_load(*ptr, "load ptr value")
            .into_pointer_value(),
        "load ptr ptr value",
    );

    let s_ext = builder.build_int_s_extend(
        char_to_put.into_int_value(),
        context.i32_type(),
        "putchar sign extend",
    );

    builder.build_call(*putchar_fn, &[s_ext.into()], "putchar call");
}

fn build_while_start<'a>(
    context: &'a Context,
    builder: &Builder,
    main_fn: &FunctionValue,
    ptr: &PointerValue,
    while_blocks: &mut VecDeque<WhileBlock<'a>>,
) {
    let num_while_blocks = while_blocks.len() + 1;
    let while_block = WhileBlock {
        while_start: context.append_basic_block(*main_fn, &format!("start{}", num_while_blocks)),
        while_body: context.append_basic_block(*main_fn, &format!("body{}", num_while_blocks)),
        while_end: context.append_basic_block(*main_fn, &format!("end{}", num_while_blocks)),
    };

    while_blocks.push_front(while_block);
    let while_block = while_blocks.front().unwrap();

    builder.build_unconditional_branch(while_block.while_start);
    builder.position_at_end(while_block.while_start);

    let ptr_load = builder.build_load(*ptr, "load ptr").into_pointer_value();

    let ptr_value = builder
        .build_load(ptr_load, "load ptr value")
        .into_int_value();

    let cmp = builder.build_int_compare(
        IntPredicate::NE,
        ptr_value,
        context.i8_type().const_int(0, false),
        "compare value at pointer to zero",
    );

    builder.build_conditional_branch(cmp, while_block.while_body, while_block.while_end);
    builder.position_at_end(while_block.while_body);
}

fn build_while_end<'a>(builder: &Builder, while_blocks: &mut VecDeque<WhileBlock<'a>>) {
    if let Some(while_block) = while_blocks.pop_front() {
        builder.build_unconditional_branch(while_block.while_start);
        builder.position_at_end(while_block.while_end);
    } else {
        panic!("Unmatched parentesis");
    } //TODO unmatched parentesis
}
