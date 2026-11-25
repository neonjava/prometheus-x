#!/usr/bin/env bash
set -euo pipefail

# Official VisA archive; override with VISA_URL or the 4th argument.
DEFAULT_VISA_URL="https://amazon-visual-anomaly.s3.us-west-2.amazonaws.com/VisA_20220922.tar"

if [ "$#" -lt 3 ]; then
    echo "Usage: $0 <category> <out_dir> <ratio> [url_or_path]" >&2
    echo "  ratio: share of good images for val and for test/good each (0 < ratio < 0.5)" >&2
    exit 1
fi

CATEGORY="$1"
OUT_DIR="$2"
VAL_RATIO="$3"
SOURCE_URL="${4:-${VISA_URL:-$DEFAULT_VISA_URL}}"

# Test needs good images too, or AUROC and false-positive rate are undefined.
if ! awk -v r="$VAL_RATIO" 'BEGIN { exit !(r ~ /^[0-9.]+$/ && r > 0 && r < 0.5) }'; then
    echo "Error: ratio must be a number with 0 < ratio < 0.5, got '$VAL_RATIO'" >&2
    exit 1
fi

# Check required tools
for cmd in curl tar awk; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "Error: required dependency '$cmd' is not installed." >&2
        exit 1
    fi
done

# Clear old splits so a re-run with another ratio cannot leave images in two splits.
rm -rf "$OUT_DIR/train" "$OUT_DIR/val" "$OUT_DIR/test"
mkdir -p "$OUT_DIR/train/good" "$OUT_DIR/val/good" "$OUT_DIR/test/good" "$OUT_DIR/test/defect"

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

# Val and test/good each get the same share; at least one image each.
HOLDOUT_COUNT=$(awk -v total="$TOTAL_GOOD" -v ratio="$VAL_RATIO" 'BEGIN { n = int(total * ratio); print (n < 1 ? 1 : n) }')
TRAIN_COUNT=$((TOTAL_GOOD - 2 * HOLDOUT_COUNT))
if [ "$TRAIN_COUNT" -lt 1 ]; then
    echo "Error: only $TOTAL_GOOD good images, too few to split with ratio $VAL_RATIO" >&2
    exit 1
fi

VAL_END=$((TRAIN_COUNT + HOLDOUT_COUNT))
for ((i=0; i<TOTAL_GOOD; i++)); do
    if [ "$i" -lt "$TRAIN_COUNT" ]; then
        dest="train/good"
    elif [ "$i" -lt "$VAL_END" ]; then
        dest="val/good"
    else
        dest="test/good"
    fi
    cp "${GOOD_FILES[i]}" "$OUT_DIR/$dest/"
done

for file in "${DEFECT_FILES[@]}"; do
    cp "$file" "$OUT_DIR/test/defect/"
done

echo "Split $CATEGORY into $OUT_DIR:"
echo "  train/good:  $TRAIN_COUNT"
echo "  val/good:    $HOLDOUT_COUNT"
echo "  test/good:   $HOLDOUT_COUNT"
echo "  test/defect: ${#DEFECT_FILES[@]}"
