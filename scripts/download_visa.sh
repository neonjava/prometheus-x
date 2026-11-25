#!/usr/bin/env bash
set -euo pipefail

# Check required CLI arguments
if [ "$#" -lt 3 ]; then
    echo "Usage: $0 <category> <out_dir> <val_ratio>" >&2
    exit 1
fi

CATEGORY="$1"
OUT_DIR="$2"
VAL_RATIO="$3"
# Allow overriding source archive URL or local path via VISA_URL or 4th arg
SOURCE_URL="${4:-${VISA_URL:-https://amazon-visual-anomaly.s3.us-west-2.amazonaws.com/VisA_20220922.tar}}"

# Check required tools
for cmd in curl tar awk; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "Error: required dependency '$cmd' is not installed." >&2
        exit 1
    fi
done

# Prepare destination directories
mkdir -p "$OUT_DIR/train/good"
mkdir -p "$OUT_DIR/val/good"
mkdir -p "$OUT_DIR/test"

# Use temporary directory for archive extraction
TMP_DIR=$(mktemp -d)
# Clean up temporary extraction folder on script exit
trap 'rm -rf "$TMP_DIR"' EXIT

ARCHIVE=""
if [ -f "$SOURCE_URL" ]; then
    ARCHIVE="$SOURCE_URL"
elif [[ "$SOURCE_URL" =~ ^file:// ]]; then
    ARCHIVE="${SOURCE_URL#file://}"
else
    # Cache archive inside OUT_DIR to avoid re-downloading across runs
    CACHE_DIR="$OUT_DIR/.cache"
    mkdir -p "$CACHE_DIR"
    ARCHIVE="$CACHE_DIR/VisA.tar"
    if [ ! -s "$ARCHIVE" ]; then
        echo "Downloading VisA dataset from $SOURCE_URL..."
        curl -fL "$SOURCE_URL" -o "$ARCHIVE"
    fi
fi

# Extract category from archive
echo "Extracting category '$CATEGORY'..."
tar -xf "$ARCHIVE" -C "$TMP_DIR" "$CATEGORY" 2>/dev/null || \
tar -xf "$ARCHIVE" -C "$TMP_DIR" "./$CATEGORY" 2>/dev/null || \
tar -xf "$ARCHIVE" -C "$TMP_DIR"

# Locate good and defect image source directories
GOOD_SRC=""
if [ -d "$TMP_DIR/$CATEGORY/Data/Images/Normal" ]; then
    GOOD_SRC="$TMP_DIR/$CATEGORY/Data/Images/Normal"
elif [ -d "$TMP_DIR/$CATEGORY/Normal" ]; then
    GOOD_SRC="$TMP_DIR/$CATEGORY/Normal"
elif [ -d "$TMP_DIR/$CATEGORY/good" ]; then
    GOOD_SRC="$TMP_DIR/$CATEGORY/good"
else
    GOOD_SRC=$(find "$TMP_DIR" -type d \( -iname "Normal" -o -iname "good" \) 2>/dev/null | head -n 1)
fi

DEFECT_SRC=""
if [ -d "$TMP_DIR/$CATEGORY/Data/Images/Anomaly" ]; then
    DEFECT_SRC="$TMP_DIR/$CATEGORY/Data/Images/Anomaly"
elif [ -d "$TMP_DIR/$CATEGORY/Anomaly" ]; then
    DEFECT_SRC="$TMP_DIR/$CATEGORY/Anomaly"
elif [ -d "$TMP_DIR/$CATEGORY/defect" ]; then
    DEFECT_SRC="$TMP_DIR/$CATEGORY/defect"
else
    DEFECT_SRC=$(find "$TMP_DIR" -type d \( -iname "Anomaly" -o -iname "defect" \) 2>/dev/null | head -n 1)
fi

if [ -z "$GOOD_SRC" ] || [ ! -d "$GOOD_SRC" ]; then
    echo "Error: good images directory not found for category '$CATEGORY'" >&2
    exit 1
fi

if [ -z "$DEFECT_SRC" ] || [ ! -d "$DEFECT_SRC" ]; then
    echo "Error: defect images directory not found for category '$CATEGORY'" >&2
    exit 1
fi

# Read sorted file lists into arrays
GOOD_FILES=()
while IFS= read -r file; do
    [ -n "$file" ] && GOOD_FILES+=("$file")
done < <(find "$GOOD_SRC" -maxdepth 1 -type f \( -iname "*.jpg" -o -iname "*.jpeg" -o -iname "*.png" \) | sort)

DEFECT_FILES=()
while IFS= read -r file; do
    [ -n "$file" ] && DEFECT_FILES+=("$file")
done < <(find "$DEFECT_SRC" -maxdepth 1 -type f \( -iname "*.jpg" -o -iname "*.jpeg" -o -iname "*.png" \) | sort)

TOTAL_GOOD=${#GOOD_FILES[@]}
if [ "$TOTAL_GOOD" -eq 0 ]; then
    echo "Error: no good images found in '$GOOD_SRC'" >&2
    exit 1
fi

# Calculate validation split size
VAL_COUNT=$(awk -v total="$TOTAL_GOOD" -v ratio="$VAL_RATIO" 'BEGIN { printf "%d\n", int(total * ratio) }')
if [ "$VAL_COUNT" -eq 0 ] && [ "$TOTAL_GOOD" -gt 1 ] && [ "$(awk -v r="$VAL_RATIO" 'BEGIN { print (r > 0 ? 1 : 0) }')" -eq 1 ]; then
    VAL_COUNT=1
fi
TRAIN_COUNT=$((TOTAL_GOOD - VAL_COUNT))

# Copy images into train and val splits
for ((i=0; i<TRAIN_COUNT; i++)); do
    cp "${GOOD_FILES[i]}" "$OUT_DIR/train/good/"
done

for ((i=TRAIN_COUNT; i<TOTAL_GOOD; i++)); do
    cp "${GOOD_FILES[i]}" "$OUT_DIR/val/good/"
done

# Copy defect images into test split
for file in "${DEFECT_FILES[@]}"; do
    cp "$file" "$OUT_DIR/test/"
done

echo "Successfully split $CATEGORY into $OUT_DIR:"
echo "  train/good: $TRAIN_COUNT images"
echo "  val/good:   $VAL_COUNT images"
echo "  test:       ${#DEFECT_FILES[@]} defect images"
