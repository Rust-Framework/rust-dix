# LRDI 过程宏深度指南

## 一、`#[derive(Inject)]` 完整指南

### 1.1 宏展开原理

`#[derive(Inject)]` 是一个派生宏（derive macro），由 `inject_derive` 入口函数驱动。其展开过程分为以下四个阶段：

**第一阶段：解析结构体定义**

`expand_inject` 函数接收 `DeriveInput`，从中提取：
- **结构体名称** (`name`)：来自 `input.ident`
- **命名字段列表** (`named`)：通过解析 `input.data` 获取

```rust
fn expand_inject(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;
    let named = match &input.data {
        syn::Data::Struct(s) => match &s.fields {
            Fields::Named(n) => n,
            _ => return Err(syn::Error::new_spanned(name, "named fields")),
        },
        _ => return Err(syn::Error::new_spanned(name, "struct only")),
    };
```

若字段不是 `Fields::Named`（例如元组结构体），产生编译错误 `"named fields"`。若数据类型不是 `struct`（例如 `enum` 或 `union`），产生 `"struct only"`。

**第二阶段：遍历字段，解析 `#[inject(...)]` 属性**

对每个字段调用 `parse_ia` 函数。该函数扫描字段上的属性并填充 `IA` 结构体：

```rust
#[derive(Default)]
struct IA {
    skip: bool,
    optional: bool,
    provider: bool,
    key: Option<String>,
}
```

`parse_ia` 实现逻辑：
1. 遍历字段的所有属性 (`f.attrs`)
2. 过滤出 `path` 为 `inject` 的属性
3. 调用 `require_list()` 确保为括号列表形式（如 `#[inject(...)]`）
4. 使用 `parse_nested_meta` 逐项解析嵌套元数据：
   - 识别 `skip` → `a.skip = true`
   - 识别 `optional` → `a.optional = true`
   - 识别 `provider` → `a.provider = true`
   - 识别 `key = "..."`
    → 解析字符串字面量，`a.key = Some("...")`

**第三阶段：为每个字段生成解析表达式**

首先调用 `saw(&field.ty)` 获取剥离包装后的内层类型（详见 1.2 节）。然后根据属性组合生成对应的代码片段。分支优先级为：

| 优先级 | 判断条件 | 生成逻辑 |
|-------|---------|---------|
| 1 | `attrs.skip == true` | `Default::default()` |
| 2 | `attrs.provider == true` | `resolver.clone()` |
| 3 | `attrs.optional == true` 且 有 key | `get_keyed_any` → downcast → `map(Arc::clone)`（返回 Option） |
| 4 | `attrs.optional == true` 且 无 key | `get_any` → downcast → `map(Arc::clone)`（返回 Option） |
| 5 | 有 key（非 optional） | `get_keyed_any` → downcast → `unwrap_or_else(panic!("keyed not found"))` |
| 6 | 无标记（默认） | `get_any` → downcast → `unwrap_or_else(panic!("svc not registered"))` |

每个分支生成的 `TokenStream` 填充到 `inits` 向量中。

**第四阶段：生成构造器函数**

将所有字段初始化表达式组装为 `__rdi_construct_{TypeName}` 函数：

```rust
#[doc(hidden)]
pub fn __rdi_construct_{Name}(resolver: &dyn lrdi::IServiceResolver)
    -> ::std::sync::Arc<{Name}>
{
    ::std::sync::Arc::new({Name} { /* 每个字段的初始化表达式 */ })
}
```

关键特征：
- 函数标记 `#[doc(hidden)]`，IDE 中可调用但不污染文档补全
- 参数固定为 `resolver: &dyn lrdi::IServiceResolver`
- 返回固定为 `Arc<TypeName>`
- 字段初始化表达式直接内联展开，零额外函数调用开销

---

### 1.2 `saw()` 函数——类型剥离

`saw()` 从字段类型中剥离外层包装，提取出内层的实际类型 `T`。

```rust
fn saw(ty: &syn::Type) -> (proc_macro2::TokenStream, bool) {
    // 返回值：(inner_type_tokens, is_arc)
}
```

**识别规则：**

| 输入类型 | 输出 `inner` | `is_arc` | 说明 |
|---------|-------------|----------|------|
| `Arc<Logger>` | `Logger` | `true` | 匹配 `Path("Arc")` 且包含泛型参数 |
| `Option<Arc<MyService>>` | `MyService` | `true` | 嵌套匹配：`Option` → `Arc` → 泛型参数 |
| `String` | `String` | `false` | 不匹配任何包装，原样返回 |
| `Arc<dyn ITrait>` | `dyn ITrait` | `true` | trait object 同样支持 |

**实现细节**（出自 `lrdi-macros/src/lib.rs`）：

```rust
fn saw(ty: &syn::Type) -> (proc_macro2::TokenStream, bool) {
    if let syn::Type::Path(p) = ty {
        let l = p.path.segments.last().unwrap();
        // 匹配 Arc<T>
        if l.ident == "Arc" {
            if let syn::PathArguments::AngleBracketed(a) = &l.arguments {
                if let Some(syn::GenericArgument::Type(i)) = a.args.first() {
                    return (quote! {#i}, true);
                }
            }
        }
        // 匹配 Option<Arc<T>>
        if l.ident == "Option" {
            if let syn::PathArguments::AngleBracketed(a) = &l.arguments {
                if let Some(syn::GenericArgument::Type(syn::Type::Path(ip))) = a.args.first() {
                    if ip.path.segments.last()
                        .map(|s| s.ident == "Arc").unwrap_or(false)
                    {
                        if let syn::PathArguments::AngleBracketed(ia) =
                            &ip.path.segments.last().unwrap().arguments
                        {
                            if let Some(syn::GenericArgument::Type(t)) = ia.args.first() {
                                return (quote! {#t}, true);
                            }
                        }
                    }
                }
            }
        }
    }
    (quote! {#ty}, false)
}
```

该函数提取的内层类型用于 `::std::any::type_name::<#inner>()` 调用，即用于类型名称字符串查找。

---

### 1.3 字段属性完整参考表

| 属性标记 | 字段类型要求 | 解析行为 | 未找到时行为 |
|---------|------------|---------|------------|
| **无标记** | `Arc<T>` | `resolver.get_any(type_name::<T>())` → downcast 到 `Arc<T>` | **panic!** `"svc not registered"` |
| `#[inject(skip)]` | 任意（需实现 `Default`） | `Default::default()` | N/A（直接使用默认值） |
| `#[inject(optional)]` | `Option<Arc<T>>` | `resolver.get_any` → downcast → `map(Arc::clone)` | 返回 `None`（不 panic） |
| `#[inject(key = "k")]` | `Arc<T>` | `resolver.get_keyed_any(type_name::<T>(), "k")` → downcast | **panic!** `"keyed not found"` |
| `#[inject(optional, key = "k")]` | `Option<Arc<T>>` | `resolver.get_keyed_any` → downcast → `map` | 返回 `None`（不 panic） |
| `#[inject(provider)]` | `Arc<dyn IServiceResolver>` | `resolver.clone()` | N/A（直接传入 resolver） |

**属性组合规则：**

- `skip` 为最高优先级，一旦设置，`optional`、`key`、`provider` 均被忽略
- `provider` 为第二优先级，设置后 `optional` 和 `key` 被忽略
- `optional` 与 `key` 可组合使用
- `skip`、`optional`、`provider` 不可同时使用（虽不会报错，但仅有最高优先级的生效）

---

### 1.4 实战：完整服务定义

以下示例展示一个包含全部 6 种字段类型的 `OrderService`：

```rust
use lrdi::*;
use lrdi_macros::Inject;
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};

// 被 skip 的类型必须实现 Default
#[derive(Default)]
struct Counter(AtomicU64);

impl Counter {
    fn next(&self) -> u64 {
        self.0.fetch_add(1, Ordering::SeqCst)
    }
}

// 带有全部 6 种字段类型的完整服务
#[derive(Inject)]
struct OrderService {
    // 1. 无标记：必须已注册，否则 panic
    logger: Arc<Logger>,

    // 2. skip：使用 Default::default()
    #[inject(skip)]
    counter: Counter,

    // 3. optional：可选的依赖，未注册时返回 None
    #[inject(optional)]
    notifier: Option<Arc<Notifier>>,

    // 4. key：按键解析，必须已注册，否则 panic
    #[inject(key = "order-logger")]
    order_logger: Arc<Logger>,

    // 5. optional + key：按键可选解析
    #[inject(optional, key = "audit")]
    audit_logger: Option<Arc<Logger>>,

    // 6. provider：直接获取 resolver
    #[inject(provider)]
    resolver: Arc<dyn IServiceResolver>,
}

#[derive(Default)]
struct Logger { prefix: String }

#[derive(Default)]
struct Notifier;

// 注册所有依赖
fn build_provider() -> Result<Arc<ServiceProvider>, RdiError> {
    Ok(Arc::new(ServiceCollection::new()
        .singleton(|_| Arc::new(Logger { prefix: "main".into() }))
        .keyed("order-logger", |_| Arc::new(Logger { prefix: "order".into() }))
        .singleton(|_| Arc::new(Notifier))
        .build()?))
}

// 解析验证
fn test_order_service() {
    let provider = build_provider().unwrap();
    let service = __rdi_construct_OrderService(&provider);

    assert_eq!(service.logger.prefix, "main");          // 无标记解析
    assert_eq!(service.counter.next(), 0);               // skip → Default
    assert!(service.notifier.is_some());                 // optional → Some
    assert_eq!(service.order_logger.prefix, "order");    // 键控解析
    assert!(service.audit_logger.is_none());             // 未注册键 → None
    // service.resolver 可直接用于后续解析
}
```

---

### 1.5 编译错误排查

| 错误信息 | 原因 | 解决方法 |
|---------|------|---------|
| `"named fields"` | 结构体不是命名字段形式（如元组结构体 `struct Foo(i32)`） | 改为 `struct Foo { field: T }` 格式 |
| `"struct only"` | 对 `enum` 或 `union` 使用了 `#[derive(Inject)]` | 仅对 `struct` 使用此宏 |
| `#[inject]` 属性解析失败 | 属性拼写错误，如 `#[inject(keyed = "...")]` | 检查属性名：仅 `skip`、`optional`、`key`、`provider` |
| `Default` trait 缺失 | 被 `#[inject(skip)]` 标记的字段类型未实现 `Default` | 为该类型实现 `Default`，如 `#[derive(Default)]` 或手动 `impl` |
| 根本原因 panic | 运行时对应服务未注册 | 检查 `ServiceCollection` 注册：类型名和 key 必须匹配；检查 provider 构建是否成功 |

---

### 1.6 生成函数命名

生成的函数名格式为 `__rdi_construct_{TypeName}`（注意前缀是 `__rdi_`，不是 `__lrdi_`）。

```rust
// 源码中：
let fn_name = format_ident!("__rdi_construct_{}", name);

// 示例展开：
#[derive(Inject)]
struct MyService { log: Arc<Logger> }

// 生成：
#[doc(hidden)]
pub fn __rdi_construct_MyService(resolver: &dyn lrdi::IServiceResolver)
    -> ::std::sync::Arc<MyService> { ... }
```

- 函数标记 `#[doc(hidden)]`，在 `cargo doc` 生成的文档中不可见
- 在 IDE 中虽然可见，但因文档隐藏不会污染自动补全列表
- 调用方式：`__rdi_construct_MyService(&resolver)` ——直接使用，无需 `use` 或完全限定路径（只要在同一模块内）

---

## 二、`#[lrdi::module]` 完整指南

### 2.1 宏展开原理

`#[lrdi::module]` 是一个属性宏（attribute macro），由 `module` 入口函数驱动，作用于 `mod` 块。

展开过程分为 7 个阶段：

**阶段 1：解析模块定义**

`expand_md` 接收 `ItemMod`，提取模块名（`m.ident`）和模块内容。若模块无 body（如 `mod foo;` 仅声明），产生错误 `"body required"`。

```rust
fn expand_md(mut m: ItemMod) -> syn::Result<proc_macro2::TokenStream> {
    let mn = m.ident.clone();
    let fn_n = format_ident!("__rdi_build_provider_{}", mn);
```

函数名固定为 `__rdi_build_provider_{module_name}`。

**阶段 2：分类模块内的 item**

遍历模块内的所有 item，将每个 item 分为两类：
- **`inject!` 调用**：匹配 `syn::Item::Macro`，且 macro path 为 `inject` 或 `lrdi::inject` → 尝试解析为 `ID` 结构体，成功则加入 `rs` 向量
- **其他 item**：所有非 `inject!` 的宏调用、函数、类型定义等 → 原样保留在 `cl` 向量中

```rust
let mut rs = Vec::new();  // 注册项列表
let mut cl = Vec::new();  // 保留项列表
for i in &is {
    match i {
        syn::Item::Macro(mc) => {
            let ps = mc.mac.path.segments.iter()
                .map(|s| s.ident.to_string()).collect::<Vec<_>>().join("::");
            if ps == "inject" || ps == "lrdi::inject" {
                if let Ok(r) = syn::parse2::<ID>(mc.mac.tokens.clone()) {
                    rs.push(r);  // ← 加入注册项
                }
            } else {
                cl.push(i.clone());  // ← 保留非 inject 宏
            }
        }
        _ => cl.push(i.clone()),  // ← 保留所有非宏 item
    }
}
```

**阶段 3：生成 ServiceCollection 链式调用**

对每个注册项 `ID`，根据其 `kind`（`IK` 枚举）生成对应的方法调用：

| `IK` 变体 | 对应 ServiceCollection 方法 | 方法选择依据 |
|----------|---------------------------|------------|
| `IK::N { lt, ty, imp }` | `lmt(lt)`（`singleton` / `scoped` / `transient`） | 生命周期 → 方法名 |
| `IK::K { key, lt, ty }` | `kmt(lt)`（`keyed` / `keyed_scoped` / `keyed_transient`） | 生命周期 → 键控方法名 |
| `IK::F { lt, f }` | `lmt(lt)`（`singleton` / `scoped` / `transient`） | 生命周期 → 方法名，使用自定义闭包 |

**阶段 4：生成 `__rdi_build_provider_*` 函数**

将 `ServiceCollection` 链式调用封装到函数中：

```rust
#[doc(hidden)]
pub fn __rdi_build_provider_{module}() -> ::std::result::Result<
    ::std::sync::Arc<lrdi::ServiceProvider>, lrdi::RdiError>
{
    Ok(::std::sync::Arc::new(
        lrdi::ServiceCollection::new()
            #(#ch)*  // ← 逐个链式调用
            .build()?
    ))
}
```

**阶段 5：重复 key 检测**

调用 `vd(&rs)` 函数，扫描所有 `IK::K` 注册项，检查是否有重复 key。详见 2.5 节。

**阶段 6：保留原模块中的非 inject item**

所有未被识别为 `inject!` 的 item 原样保留在模块中。

**阶段 7：输出完整模块**

将保留的 item 与新生成的函数合并，输出改写后的 `ItemMod`：

```rust
cl.push(bi);  // 将构建函数加入保留项
m.content = Some((syn::token::Brace::default(), cl));
Ok(quote! {#m})
```

---

### 2.2 `IK` 枚举——所有声明类型

`IK` 枚举定义在宏内部（非公开），表示三种注册模式：

```rust
enum IK {
    N { lt: LT, ty: syn::Type, imp: Option<syn::Type> },
    K { key: String, lt: LT, ty: syn::Type },
    F { lt: LT, f: syn::Expr },
}
```

| 变体 | 含义 | 对应语法 | 生成的 ServiceCollection 方法 |
|------|------|---------|---------------------------|
| `IK::N` | 普通注册 | `singleton: MyType` | `.singleton(\|_\| Arc::new(MyType::default()))` |
| `IK::N` (含 imp) | 接口实现注册 | `singleton: dyn ITrait => MyImpl` | `.singleton::<dyn ITrait>(\|_\| Arc::new(MyImpl::default()))` |
| `IK::K` | 键控注册 | `keyed "k": singleton: MyType` | `.keyed::<MyType>("k", \|_\| ...)` |
| `IK::F` | 自定义工厂 | `factory singleton: Type => expr` | `.singleton(move \|_\| Arc::new(expr))` |

**字段详解：**

- `lt: LT`：生命周期标签，决定方法名（见 2.3 节）
- `ty: syn::Type`：服务的具体类型
- `imp: Option<syn::Type>`：（仅 `IK::N`）`dyn Trait` 的实现类型，若为 `Some` 则生成 `dyn Trait` → `ImplType` 映射
- `key: String`：（仅 `IK::K`）键名
- `f: syn::Expr`：（仅 `IK::F`）闭包体表达式

---

### 2.3 `LT` 枚举——生命周期解析

`LT` 枚举实现 `syn::Parse`，将标识符映射为生命周期：

```rust
enum LT { S, Sc, T }

impl Parse for LT {
    fn parse(i: ParseStream) -> syn::Result<Self> {
        match i.parse::<syn::Ident>()?.to_string().as_str() {
            "singleton" => Ok(LT::S),
            "scoped"    => Ok(LT::Sc),
            "transient" => Ok(LT::T),
            o => Err(syn::Error::new(i.span(), format!("unknown lifetime: {o}"))),
        }
    }
}
```

**对应关系：**

| 输入标识符 | `LT` 变体 | 普通注册方法 (`lmt`) | 键控注册方法 (`kmt`) |
|-----------|----------|-------------------|-------------------|
| `singleton` | `LT::S` | `.singleton(...)` | `.keyed(...)` |
| `scoped` | `LT::Sc` | `.scoped(...)` | `.keyed_scoped(...)` |
| `transient` | `LT::T` | `.transient(...)` | `.keyed_transient(...)` |

`lmt()` 和 `kmt()` 函数将 `LT` 枚举还原为对应的方法标识符：

```rust
fn lmt(lt: LT) -> proc_macro2::TokenStream {
    match lt {
        LT::S  => quote! {singleton},
        LT::Sc => quote! {scoped},
        LT::T  => quote! {transient},
    }
}

fn kmt(lt: LT) -> proc_macro2::TokenStream {
    match lt {
        LT::S  => quote! {keyed},
        LT::Sc => quote! {keyed_scoped},
        LT::T  => quote! {keyed_transient},
    }
}
```

---

### 2.4 声明语法完整参考

#### 语法 1：普通 singleton 注册

```rust
lrdi::inject!(singleton: MyService);
```

等价于：
```rust
ServiceCollection::new()
    .singleton(|_: &dyn IServiceResolver| Arc::new(MyService::default()))
```

要求 `MyService` 实现 `Default`。

#### 语法 2：scoped 注册

```rust
lrdi::inject!(scoped: RequestContext);
```

每次从 scope 中解析时重新创建实例。

#### 语法 3：transient 注册

```rust
lrdi::inject!(transient: UuidGenerator);
```

每次解析都创建新实例。

#### 语法 4：dyn Trait → Impl 映射

```rust
lrdi::inject!(singleton: dyn IPlugin => TestPlugin);
```

生成 `.singleton::<dyn IPlugin>(|_| Arc::new(TestPlugin::default()))`。`TestPlugin` 必须实现 `IPlugin + Default`。

#### 语法 5：键控注册

```rust
lrdi::inject!(keyed "special": singleton: Logger);
lrdi::inject!(keyed "temp": scoped: Logger);
lrdi::inject!(keyed "per-call": transient: Logger);
```

分别对应 `.keyed("special", ...)`、`.keyed_scoped("temp", ...)`、`.keyed_transient("per-call", ...)`。

#### 语法 6：自定义工厂

```rust
lrdi::inject!(factory singleton: Logger => Logger { prefix: "factory".into() });
```

生成 `.singleton(move |_: &dyn IServiceResolver| Arc::new(Logger { prefix: "factory".into() }))`。闭包体可以是任意表达式。

支持的 factory 生命周期：`factory singleton:`、`factory scoped:`、`factory transient:`。

#### 语法 7：inject! 路径形式

```rust
// 在 #[lrdi::module] 内部，两种写法等价：
inject!(singleton: MyService);
lrdi::inject!(singleton: MyService);
```

宏通过检查 macro path 是否匹配 `"inject"` 或 `"lrdi::inject"` 来判断：

```rust
if ps == "inject" || ps == "lrdi::inject" { ... }
```

---

### 2.5 `vd()` 函数——重复 key 检测

`vd()` 函数（validate dedup）在代码生成前扫描所有注册项，检测键控注册中是否有重复的 key 值：

```rust
fn vd(rs: &[ID]) -> syn::Result<()> {
    let mut sn: std::collections::HashMap<String, usize> = HashMap::new();
    for r in rs {
        if let IK::K { key, .. } = &r.kind {
            let e = sn.entry(key.clone()).or_default();
            *e += 1;
            if *e > 1 {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!("rdi-E004: duplicate key `{key}`"),
                ));
            }
        }
    }
    Ok(())
}
```

**检测逻辑：**
1. 创建空的 `HashMap<String, usize>`
2. 遍历所有注册项，仅处理 `IK::K` 变体
3. 记录每个 key 的出现次数
4. 若出现次数 > 1，返回编译错误 `rdi-E004: duplicate key \`{key}\``

**示例 —— 触发编译错误：**

```rust
#[lrdi::module]
mod broken {
    lrdi::inject!(keyed "same": singleton: Logger);
    lrdi::inject!(keyed "same": singleton: Logger);  // ← 编译错误！
}
```

编译输出：
```
error: rdi-E004: duplicate key `same`
```

---

### 2.6 实际生成代码示例

**输入模块：**

```rust
#[lrdi::module]
mod my_services {
    lrdi::inject!(singleton: Logger);
    lrdi::inject!(singleton: dyn IPlugin => TestPlugin);
    lrdi::inject!(keyed "special": singleton: Logger);
    lrdi::inject!(factory singleton: Config => Config { env: "prod".into() });

    // 普通函数会被保留
    fn helper() -> &'static str { "hello" }
}
```

**展开后的等效代码：**

```rust
mod my_services {
    // 保留的普通函数
    fn helper() -> &'static str { "hello" }

    // 生成的构建函数
    #[doc(hidden)]
    pub fn __rdi_build_provider_my_services()
        -> ::std::result::Result<
            ::std::sync::Arc<lrdi::ServiceProvider>, lrdi::RdiError>
    {
        Ok(::std::sync::Arc::new(
            lrdi::ServiceCollection::new()
                .singleton(|_: &dyn lrdi::IServiceResolver|
                    ::std::sync::Arc::new(<Logger as ::std::default::Default>::default()))
                .singleton::<dyn IPlugin>(|_: &dyn lrdi::IServiceResolver|
                    ::std::sync::Arc::new(<TestPlugin as ::std::default::Default>::default()))
                .keyed::<Logger>("special", |_: &dyn lrdi::IServiceResolver|
                    ::std::sync::Arc::new(<Logger as ::std::default::Default>::default()))
                .singleton(move |_: &dyn lrdi::IServiceResolver|
                    ::std::sync::Arc::new(Config { env: "prod".into() }))
                .build()?
        ))
    }
}
```

**使用方式：**

```rust
let provider = my_services::__rdi_build_provider_my_services().unwrap();
let logger: Arc<Logger> = provider.get::<Logger>();
let plugin: Arc<dyn IPlugin> = provider.get::<dyn IPlugin>();
let special_logger: Arc<Logger> = provider.get_keyed::<Logger>("special");
```

---

### 2.7 与手动 `ServiceCollection` 的取舍

#### 何时使用 `#[lrdi::module]`

| 场景 | 优势 |
|------|------|
| **声明式配置** | 注册项集中声明在模块中，一目了然 |
| **就近配置** | 注册代码和实现代码在同一模块，减少跳转 |
| **库/插件导出** | 每个 `mod` 自动生成 `build` 函数，适合插件按模块组织 |
| **简单工厂** | 仅需 `Default` 的注册项，无需手写闭包 |
| **键控服务批量注册** | 语法简洁，重复 key 自动检测 |

#### 何时使用手动 `ServiceCollection`

| 场景 | 必要性 |
|------|-------|
| **复杂工厂逻辑** | 需要依赖注入、条件判断、外部配置读取的工厂闭包 |
| **动态注册** | 运行时根据配置/环境决定注册哪些服务 |
| **条件注册** | 使用 `try_add` 等条件方法 |
| **非 `Default` 构造** | 工厂需要传入参数，`#[lrdi::module]` 的 factory 语法可支持但不如手写闭包灵活 |
| **需要 `instance()` / `singleton_value()`** | 直接注入已有实例 |

#### 推荐组合策略

在同一个项目中，三种方式可以自由组合：

```rust
// 1. #[lrdi::module] 用于模块级服务集合
#[lrdi::module]
mod infra {
    lrdi::inject!(singleton: Logger);
    lrdi::inject!(singleton: Config);
}

// 2. 手动 ServiceCollection 用于动态/条件注册
fn build_with_feature_toggle(enable_cache: bool) -> Result<Arc<ServiceProvider>, RdiError> {
    let base = ServiceCollection::new()
        .singleton(|_| Arc::new(Logger { prefix: "app".into() }));
    if enable_cache {
        base.singleton(|_| Arc::new(RedisCache::connect("redis://localhost")))
            .build()
    } else {
        base.singleton(|_| Arc::new(MemoryCache::default()))
            .build()
    }
}
```

---

## 三、宏混合使用策略

### 3.1 三种方式对比

| 维度 | 手动工厂 | `#[derive(Inject)]` | `#[lrdi::module]` |
|------|---------|-------------------|-------------------|
| **控制粒度** | 最细，完全可控 | 消除构造器样板 | 消除注册样板 |
| **代码量** | 最多 | 中等（需手写注册） | 最少 |
| **灵活性** | 最高 | 中等（需配合 `#[inject(provider)]`） | 较低（局限于 Default 构造） |
| **编译期安全** | 运行时发现缺失 | 运行时 panic 缺失 | 编译期检测重复 key |
| **适用场景** | 复杂依赖链 | 领域模型、服务类 | 基础设施、默认配置 |

### 3.2 推荐架构组合

```
┌─────────────────────────────────────────────────────┐
│  应用层（组合根）                                      │
│  ┌─────────────────────────────────────────────────┐ │
│  │  手动 ServiceCollection.build()                 │ │
│  │  - 动态注册                                      │ │
│  │  - 条件注册                                      │ │
│  │  - 生命周期精细控制                                │ │
│  └─────────────────────────────────────────────────┘ │
│                        │                              │
│  ┌─────────────────────▼───────────────────────────┐ │
│  │  模块层                                           │ │
│  │  #[lrdi::module] mod infra_module { ... }      │ │
│  │  #[lrdi::module] mod plugin_module { ... }     │ │
│  └─────────────────────────────────────────────────┘ │
│                        │                              │
│  ┌─────────────────────▼───────────────────────────┐ │
│  │  领域层                                           │ │
│  │  #[derive(Inject)] struct OrderService { ... }  │ │
│  │  #[derive(Inject)] struct UserService { ... }   │ │
│  └─────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────┘
```

### 3.3 完整混合示例

```rust
use lrdi::*;
use lrdi_macros::Inject;
use std::sync::Arc;

// ─── 类型定义 ─────────────────────────────────────

#[derive(Default)]
struct DatabaseConfig { url: String }

#[derive(Default)]
struct CacheConfig { ttl_secs: u64 }

#[derive(Default)]
struct Logger { prefix: String }

// ─── 领域服务：使用 #[derive(Inject)] ──────────────

#[derive(Inject)]
struct OrderProcessor {
    logger: Arc<Logger>,
    db_config: Arc<DatabaseConfig>,
    #[inject(optional)]
    cache_config: Option<Arc<CacheConfig>>,
}

// ─── 基础设施模块：使用 #[lrdi::module] ────────────

#[lrdi::module]
mod infra {
    lrdi::inject!(singleton: super::Logger);
    lrdi::inject!(singleton: super::DatabaseConfig);
}

// ─── 组合根：手动 ServiceCollection ────────────────

fn build_app(use_cache: bool) -> Result<Arc<ServiceProvider>, RdiError> {
    // 先获取基础设施 provider
    let infra_provider = infra::__rdi_build_provider_infra()?;

    // 手动构建，条件注册
    let mut col = ServiceCollection::new()
        .singleton(|_| Arc::clone(&infra_provider.get::<Logger>()))
        .singleton(|_| Arc::clone(&infra_provider.get::<DatabaseConfig>()));

    if use_cache {
        col = col.singleton(|_| Arc::new(CacheConfig { ttl_secs: 300 }));
    }

    Arc::new(col.build()?)
}

fn main() -> Result<(), RdiError> {
    let provider = build_app(true)?;
    let order_proc = __rdi_construct_OrderProcessor(&provider);

    assert_eq!(order_proc.logger.prefix, "");
    assert!(order_proc.cache_config.is_some());  // 因 use_cache=true
    Ok(())
}
```

---

## 四、源码映射速查表

| 宏/概念 | 源码位置 (`crates/lrdi-macros/src/lib.rs`) | 关键类型/函数 |
|--------|------------------------------------------|-------------|
| `#[derive(Inject)]` 入口 | 第 10-15 行 | `inject_derive()` |
| 核心展开逻辑 | 第 17-52 行 | `expand_inject()` |
| 类型剥离 | 第 54-87 行 | `saw()` |
| 属性解析 | 第 96-120 行 | `parse_ia()` |
| `IA` 结构体 | 第 89-95 行 | `IA { skip, optional, provider, key }` |
| `#[lrdi::module]` 入口 | 第 128-132 行 | `module()` |
| 模块展开逻辑 | 第 134-199 行 | `expand_md()` |
| `ID` / `IK` / `LT` | 第 232-314 行 | 注册项解析和生命周期枚举 |
| 生命周期→方法映射 | 第 200-213 行 | `lmt()`, `kmt()` |
| 重复 key 检测 | 第 214-229 行 | `vd()` |
| `inject!` 占位宏 | 第 122-125 行 | 仅返回空 TokenStream |
