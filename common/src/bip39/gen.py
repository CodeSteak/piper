
with open('bip39.txt', 'r') as f:
    words = f.readlines()

    print('pub static WORDS: [&str; 2048] = [')
    for word in words:
        print(f'    "{word.strip()}",')
    print('];') 