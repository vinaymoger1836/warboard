# Geodata build image (doc 05 §7). GDAL, Python and the pmtiles CLI, all pinned.
# Build: bash geodata/run.sh image      Use: bash geodata/run.sh build <pack>
FROM ghcr.io/osgeo/gdal:ubuntu-small-3.13.3@sha256:64250faf833c06d4b21afce4c27190039ba7ab58d70f0eebc87cf77d929c0b40

ARG PMTILES_VERSION=1.31.2
ARG PMTILES_SHA256=3ed7dbf4ec2e6dfe5e25b6f70d1ffc932729f93c86db353bf514dd71010a312f

RUN apt-get update \
 && apt-get install -y --no-install-recommends python3-pip curl ca-certificates \
 && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL -o /tmp/pmtiles.tgz \
      "https://github.com/protomaps/go-pmtiles/releases/download/v${PMTILES_VERSION}/go-pmtiles_${PMTILES_VERSION}_Linux_x86_64.tar.gz" \
 && echo "${PMTILES_SHA256}  /tmp/pmtiles.tgz" | sha256sum -c - \
 && tar -xzf /tmp/pmtiles.tgz -C /usr/local/bin pmtiles \
 && rm /tmp/pmtiles.tgz

COPY requirements.txt /tmp/requirements.txt
RUN pip install --no-cache-dir --break-system-packages -r /tmp/requirements.txt

# Deterministic GDAL: one thread, no timestamps from the environment.
ENV GDAL_NUM_THREADS=1 \
    AWS_NO_SIGN_REQUEST=YES \
    GDAL_DISABLE_READDIR_ON_OPEN=EMPTY_DIR \
    CPL_VSIL_CURL_ALLOWED_EXTENSIONS=.tif \
    GDAL_HTTP_MAX_RETRY=8 \
    GDAL_HTTP_RETRY_DELAY=2 \
    PYTHONUNBUFFERED=1 \
    PYTHONHASHSEED=0

WORKDIR /repo
ENTRYPOINT ["python3", "-m", "geodata"]
