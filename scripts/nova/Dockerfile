# Independent bounded NoVa feasibility NLP — external solver image.
# Provides: pyomo + ipopt (local NLP) via conda-forge. Couenne/BARON (global proof)
# are added later via IDAES get-extensions when needed for an infeasibility proof.
FROM continuumio/miniconda3:latest

# ipopt (conda-forge) links against libgomp (OpenMP); the slim base lacks it.
RUN apt-get update && apt-get install -y --no-install-recommends libgomp1 \
    && rm -rf /var/lib/apt/lists/*

RUN conda install -y -c conda-forge pyomo ipopt \
    && conda clean -afy

RUN ipopt --version && python3 -c "import pyomo; print('pyomo', pyomo.version.version)"

WORKDIR /work
ENTRYPOINT ["python3"]
