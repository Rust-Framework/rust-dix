"""Fix test files: add .unwrap() to get/get_keyed/get_owned/get_keyed_owned calls."""
import os
import re
import glob

TEST_DIR = r"d:\GitCode\RF\rust-di\crates\core\tests"

# Each pattern: (regex, replacement_template)
# Using negative lookbehind (?<!\w) to ensure we don't match try_get, etc.
# And checking that .unwrap() or .expect() doesn't already follow
PATTERNS = [
    # .get_keyed_owned::<T>("...") -> .get_keyed_owned::<T>("...").unwrap()
    (r'(\.get_keyed_owned::<[^>]+>\([^)]+\))', r'\1.unwrap()'),
    # .get_keyed_owned("...") -> .get_keyed_owned("...").unwrap()
    (r'(\.get_keyed_owned\("[^"]*"\))', r'\1.unwrap()'),
    # .get_owned::<T>() -> .get_owned::<T>().unwrap()
    (r'(\.get_owned::<[^>]+>\(\))', r'\1.unwrap()'),
    # .get_owned() -> .get_owned().unwrap()
    (r'(\.get_owned\(\))', r'\1.unwrap()'),
    # .get_keyed::<T>("...") -> .get_keyed::<T>("...").unwrap()
    (r'(\.get_keyed::<[^>]+>\([^)]+\))', r'\1.unwrap()'),
    # .get_keyed("...") -> .get_keyed("...").unwrap()
    (r'(\.get_keyed\("[^"]*"\))', r'\1.unwrap()'),
    # .get::<T>() -> .get::<T>().unwrap()
    (r'(\.get::<[^>]+>\(\))', r'\1.unwrap()'),
    # .get() -> .get().unwrap()
    (r'(\.get\(\))', r'\1.unwrap()'),
]

def fix_file(filepath):
    with open(filepath, 'r', encoding='utf-8') as f:
        content = f.read()

    original = content
    for pattern, replacement in PATTERNS:
        # Only replace if not already followed by .unwrap() or .expect(
        def replacer(m):
            full = m.group(0)
            end = m.end()
            rest = content[end:end+20]
            if rest.startswith('.unwrap()') or rest.startswith('.expect('):
                return full
            return full + '.unwrap()'
        
        content = re.sub(pattern, replacer, content)

    if content != original:
        with open(filepath, 'w', encoding='utf-8') as f:
            f.write(content)
        print(f"Fixed: {os.path.basename(filepath)}")

for filepath in glob.glob(f"{TEST_DIR}/*.rs"):
    fix_file(filepath)
print("Done")