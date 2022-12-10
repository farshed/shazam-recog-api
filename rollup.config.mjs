// import nodeResolve from '@rollup/plugin-node-resolve';
// import terser from '@rollup/plugin-terser';
import dts from 'rollup-plugin-dts';
import typescript from '@rollup/plugin-typescript';
import url from '@rollup/plugin-url';

const LIBRARY_NAME = 'Library';
const EXTERNAL = [];
const GLOBALS = {};

const makeConfig = (env = 'development') => {
	let bundleSuffix = '';

	if (env === 'production') {
		bundleSuffix = 'min.';
	}

	return {
		input: 'lib.ts',
		external: EXTERNAL,
		output: [
			{
				name: LIBRARY_NAME,
				file: `dist/${LIBRARY_NAME}.umd.${bundleSuffix}js`,
				format: 'umd',
				exports: 'auto',
				globals: GLOBALS
			},
			{
				file: `dist/${LIBRARY_NAME}.cjs.${bundleSuffix}js`,
				format: 'cjs',
				exports: 'auto',
				globals: GLOBALS
			},
			{
				file: `dist/${LIBRARY_NAME}.esm.${bundleSuffix}js`,
				format: 'es',
				exports: 'named',
				globals: GLOBALS
			}
		],
		plugins: [
			typescript(),
			// nodeResolve(),
			url({ include: ['**/*.wasm'], limit: 14336000 })
			// ...(env === 'production' ? [terser()] : [])
		]
	};
};

export default (commandLineArgs) => {
	const configs = [
		makeConfig(),
		{
			input: './pkg/shazam_fpgen.d.ts',
			output: [{ file: 'dist/pkg/shazam_fpgen.d.ts', format: 'es' }],
			plugins: [dts()]
		}
	];

	if (commandLineArgs.environment === 'BUILD:production') {
		configs.push(makeConfig('production'));
	}

	return configs;
};
