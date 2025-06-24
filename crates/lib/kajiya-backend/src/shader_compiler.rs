use crate::file::LoadFile;
use anyhow::{Context, Result, anyhow, bail};
use bytes::Bytes;
use std::{path::PathBuf, sync::Arc};
use turbosloth::*;

pub struct CompiledShader {
    pub name: String,
    pub spirv: Bytes,
}

#[derive(Clone, Hash)]
pub struct CompileShader {
    pub path: PathBuf,
    pub profile: String,
}

#[async_trait]
impl LazyWorker for CompileShader {
    type Output = Result<CompiledShader>;

    async fn run(self, ctx: RunContext) -> Self::Output {
        let ext = self
            .path
            .extension()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "".to_string());

        let name = self
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        match ext.as_str() {
            "glsl" => unimplemented!(),
            "spv" => {
                let spirv = LoadFile::new(self.path.clone())?.run(ctx).await?;
                Ok(CompiledShader { name, spirv })
            }
            "hlsl" => {
                let file_path = self.path.to_str().unwrap().to_owned();
                let source = shader_prepper::process_file(
                    &file_path,
                    &mut ShaderIncludeProvider { ctx },
                    String::new(),
                );
                let source = source
                    .map_err(|err| anyhow!("{}", err))
                    .with_context(|| format!("shader path: {:?}", self.path))?;
                let target_profile = format!("{}_6_4", self.profile);
                let spirv = compile_generic_shader_hlsl_impl(&name, &source, &target_profile)?;

                Ok(CompiledShader { name, spirv })
            }
            _ => anyhow::bail!("Unrecognized shader file extension: {}", ext),
        }
    }
}

pub struct RayTracingShader {
    pub name: String,
    pub spirv: Bytes,
}

#[derive(Clone, Hash)]
pub struct CompileRayTracingShader {
    pub path: PathBuf,
}

#[async_trait]
impl LazyWorker for CompileRayTracingShader {
    type Output = Result<RayTracingShader>;

    async fn run(self, ctx: RunContext) -> Self::Output {
        let file_path = self.path.to_str().unwrap().to_owned();
        let source = shader_prepper::process_file(
            &file_path,
            &mut ShaderIncludeProvider { ctx },
            String::new(),
        );
        let source = source.map_err(|err| anyhow!("{}", err))?;

        let ext = self
            .path
            .extension()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "".to_string());

        let name = self
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        match ext.as_str() {
            "glsl" => unimplemented!(),
            "hlsl" => {
                let target_profile = "lib_6_4";
                let spirv = compile_generic_shader_hlsl_impl(&name, &source, target_profile)?;

                Ok(RayTracingShader { name, spirv })
            }
            _ => anyhow::bail!("Unrecognized shader file extension: {}", ext),
        }
    }
}

struct ShaderIncludeProvider {
    ctx: RunContext,
}

impl shader_prepper::IncludeProvider for ShaderIncludeProvider {
    type IncludeContext = String;

    fn resolve_path(
        &self,
        path: &str,
        context: &Self::IncludeContext,
    ) -> std::result::Result<
        shader_prepper::ResolvedInclude<Self::IncludeContext>,
        shader_prepper::BoxedIncludeProviderError,
    > {
        let context = context.clone();
        Ok(shader_prepper::ResolvedInclude {
            resolved_path: shader_prepper::ResolvedIncludePath(path.to_owned()),
            context: context,
        })
    }

    fn get_include(
        &mut self,
        path: &shader_prepper::ResolvedIncludePath,
    ) -> Result<String, shader_prepper::BoxedIncludeProviderError> {
        let resolved_path = if let Some('/') = path.0.chars().next() {
            &path.to_owned()
        } else {
            path
        };

        let blob: Arc<Bytes> = smol::block_on(
            crate::file::LoadFile::new(&resolved_path.0)
                .with_context(|| format!("Failed loading shader include {}", path.0))?
                .into_lazy()
                .eval(&self.ctx),
        )?;

        Ok(String::from_utf8(blob.to_vec())?)
    }
}

pub fn get_cs_local_size_from_spirv(spirv: &[u32]) -> Result<[u32; 3]> {
    let mut loader = rspirv::dr::Loader::new();
    rspirv::binary::parse_words(spirv, &mut loader).unwrap();
    let module = loader.module();

    for inst in module.global_inst_iter() {
        //if spirv_headers::Op::ExecutionMode == inst.class.opcode {
        if inst.class.opcode as u32 == 16 {
            let local_size = &inst.operands[2..5];
            use rspirv::dr::Operand::LiteralBit32;

            if let [LiteralBit32(x), LiteralBit32(y), LiteralBit32(z)] = *local_size {
                return Ok([x, y, z]);
            } else {
                bail!("Could not parse the ExecutionMode SPIR-V op");
            }
        }
    }

    Err(anyhow!("Could not find a ExecutionMode SPIR-V op"))
}

fn compile_generic_shader_hlsl_impl(
    name: &str,
    source: &[shader_prepper::SourceChunk<String>],
    target_profile: &str,
) -> Result<Bytes> {
    let mut source_text = String::new();
    for s in source {
        source_text += &s.source;
    }

    let t0 = std::time::Instant::now();
    let spirv = hassle_rs::compile_hlsl(
        name,
        &source_text,
        "main",
        target_profile,
        &[
            "-spirv",
            //"-enable-16bit-types",
            "-fspv-target-env=vulkan1.2",
            "-WX",      // warnings as errors
            "-Ges",     // strict mode
            "-HV 2021", // HLSL version 2021
        ],
        &[],
    )
    .map_err(|err| anyhow!("{}", err))?;

    log::trace!("dxc took {:?} for {}", t0.elapsed(), name,);

    Ok(spirv.into())
}
